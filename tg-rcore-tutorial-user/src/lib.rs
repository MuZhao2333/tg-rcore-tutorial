#![no_std]
#![allow(dead_code)]
//!
//! 教程阅读建议：
//!
//! - `_start` 展示用户程序最小运行时（初始化控制台、堆、调用 main）；
//! - 其余辅助函数（sleep/pipe_*）展示了常见 syscall 组合用法。

mod heap;

extern crate alloc;

use tg_console::log;

pub use tg_console::{print, println};
pub use tg_syscall::*;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
pub extern "C" fn _start() -> ! {
    // 用户态运行时初始化顺序与内核类似：先 I/O，再堆，再进入 main。
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    heap::init();

    unsafe extern "C" {
        fn main() -> i32;
    }

    // SAFETY: main 函数由用户程序提供，链接器保证其存在且符合 C ABI
    exit(unsafe { main() });
    unreachable!()
}

#[panic_handler]
fn panic_handler(panic_info: &core::panic::PanicInfo) -> ! {
    let err = panic_info.message();
    if let Some(location) = panic_info.location() {
        log::error!("Panicked at {}:{}, {err}", location.file(), location.line());
    } else {
        log::error!("Panicked: {err}");
    }
    exit(1);
    unreachable!()
}

// ============================================================================
// Framebuffer / GPU 接口（供 C 代码调用）
// ============================================================================

#[repr(C)]
pub struct TgFramebufferInfo {
    pub ptr: *mut u8,
    /// 映射区域总字节数（pitch * height，含行填充）
    pub len: usize,
    pub width: usize,
    pub height: usize,
    /// 每行字节数（VirtIO 可能大于 width * 4）
    pub pitch: usize,
}

/// 获取 framebuffer 信息
#[unsafe(no_mangle)]
pub extern "C" fn tg_framebuffer_info(out: *mut TgFramebufferInfo) -> i32 {
    if out.is_null() {
        return -1;
    }
    #[cfg(target_arch = "riscv64")]
    {
        let mut info = FbInfo {
            ptr: 0,
            width: 0,
            height: 0,
            pitch: 0,
            format: 0,
        };
        let ret = get_fb_info(&mut info);
        if ret < 0 || info.ptr == 0 || info.width == 0 || info.height == 0 {
            return -1;
        }
        let pitch = if info.pitch != 0 {
            info.pitch as usize
        } else {
            info.width as usize * 4
        };
        let len = pitch.saturating_mul(info.height as usize);
        unsafe {
            (*out).ptr = info.ptr as *mut u8;
            (*out).len = len;
            (*out).width = info.width as usize;
            (*out).height = info.height as usize;
            (*out).pitch = pitch;
        }
        0
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = out;
        -1
    }
}

/// 刷新 framebuffer
#[unsafe(no_mangle)]
pub extern "C" fn tg_framebuffer_flush() -> i32 {
    #[cfg(target_arch = "riscv64")]
    {
        framebuffer_flush() as i32
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        -1
    }
}

/// 设置输入模式（0=轮询模式）- 简化实现
#[unsafe(no_mangle)]
pub extern "C" fn tg_set_input_mode_polling() -> i32 {
    0 // 成功
}

// ============================================================================
// 时间接口
// ============================================================================

/// 获取当前时间（毫秒）
#[unsafe(no_mangle)]
pub extern "C" fn tg_get_ticks_ms() -> u32 {
    let mut time: TimeSpec = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    (time.tv_sec * 1000 + time.tv_nsec / 1_000_000) as u32
}

/// 睡眠（毫秒）
#[unsafe(no_mangle)]
pub extern "C" fn tg_sleep_ms(ms: u32) {
    let mut time: TimeSpec = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    let deadline = time + TimeSpec::from_millsecond(ms as usize);
    loop {
        let mut now: TimeSpec = TimeSpec::ZERO;
        clock_gettime(ClockId::CLOCK_MONOTONIC, &mut now as *mut _ as _);
        if now >= deadline {
            break;
        }
        sched_yield();
    }
}

// ============================================================================
// 输入接口
// ============================================================================

/// 非阻塞获取键盘输入（返回 ASCII 字符，-1 表示无输入）
#[unsafe(no_mangle)]
pub extern "C" fn tg_getchar_poll() -> i32 {
    let mut c = [0u8; 1];
    match read(STDIN, &mut c) {
        1 => c[0] as i32,
        _ => -1,
    }
}

// ============================================================================
// 文件操作接口（供 C libc shim 调用）
// ============================================================================

const O_RDONLY: i32 = 0;
const O_WRONLY: i32 = 1;
const O_RDWR: i32 = 2;
const O_CREATE: i32 = 0x40;
const O_TRUNC: i32 = 0x200;

fn flags_from_i32(flags: i32) -> OpenFlags {
    let mut result = OpenFlags::empty();
    match flags & 3 {
        0 => result.insert(OpenFlags::RDONLY),
        1 => result.insert(OpenFlags::WRONLY),
        2 => result.insert(OpenFlags::RDWR),
        _ => {}
    }
    if (flags & O_CREATE) != 0 {
        result.insert(OpenFlags::CREATE);
    }
    if (flags & O_TRUNC) != 0 {
        result.insert(OpenFlags::TRUNC);
    }
    result
}

#[unsafe(no_mangle)]
pub extern "C" fn tg_sys_open(path: *const u8, flags: i32) -> i32 {
    if path.is_null() {
        return -1;
    }
    let mut len = 0usize;
    unsafe {
        while *path.add(len) != 0 {
            len += 1;
        }
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path, len) };
    let path_str = unsafe { core::str::from_utf8_unchecked(path_slice) };
    open(path_str, flags_from_i32(flags)) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn tg_sys_close(fd: i32) -> i32 {
    if fd < 0 {
        return -1;
    }
    close(fd as usize) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn tg_sys_read(fd: i32, buf: *mut u8, len: usize) -> i32 {
    if fd < 0 || buf.is_null() {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    read(fd as usize, data) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn tg_sys_write(fd: i32, buf: *const u8, len: usize) -> i32 {
    if fd < 0 || buf.is_null() {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts(buf, len) };
    write(fd as usize, data) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn tg_sys_unlink(path: *const u8) -> i32 {
    if path.is_null() {
        return -1;
    }
    let mut len = 0usize;
    unsafe {
        while *path.add(len) != 0 {
            len += 1;
        }
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path, len) };
    let path_str = unsafe { core::str::from_utf8_unchecked(path_slice) };
    unlink(path_str) as i32
}

// ============================================================================
// 内存分配接口
// ============================================================================

const TG_ALLOC_ARENA_SIZE: usize = 16 << 20;

#[repr(align(16))]
struct TgAllocArena([u8; TG_ALLOC_ARENA_SIZE]);

static mut TG_ALLOC_ARENA: TgAllocArena = TgAllocArena([0; TG_ALLOC_ARENA_SIZE]);
static TG_ALLOC_OFFSET: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn tg_alloc(size: usize) -> *mut u8 {
    let req = size.max(1);
    let align_mask = 8usize - 1;
    loop {
        let current = TG_ALLOC_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
        let aligned = (current + align_mask) & !align_mask;
        let Some(next) = aligned.checked_add(req) else {
            return core::ptr::null_mut();
        };
        if next > TG_ALLOC_ARENA_SIZE {
            return core::ptr::null_mut();
        }
        if TG_ALLOC_OFFSET
            .compare_exchange(
                current,
                next,
                core::sync::atomic::Ordering::SeqCst,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            let base = core::ptr::addr_of_mut!(TG_ALLOC_ARENA) as *mut u8;
            return unsafe { base.add(aligned) };
        }
    }
}

// ============================================================================
// DOOM generic 兼容层（供 Rust demo 和 C 代码共用）
// ============================================================================

pub mod doomgeneric {
    use crate::*;

    pub const DG_SCREEN_WIDTH: usize = 320;
    pub const DG_SCREEN_HEIGHT: usize = 200;
    pub const DOOM_BUFFER_SIZE: usize = DG_SCREEN_WIDTH * DG_SCREEN_HEIGHT;

    pub static mut DOOM_SCREEN_BUFFER: [u8; DOOM_BUFFER_SIZE] =
        [0u8; DOOM_BUFFER_SIZE];
    pub static mut DG_INITIALIZED: bool = false;

    pub fn dg_init() {
        unsafe {
            if DG_INITIALIZED {
                return;
            }
            DG_INITIALIZED = true;
        }
    }

    pub fn dg_get_screen_buffer() -> *mut u8 {
        unsafe { core::ptr::addr_of_mut!(DOOM_SCREEN_BUFFER) as *mut u8 }
    }

    pub fn dg_set_pixel(x: usize, y: usize, color_idx: u8) {
        if x >= DG_SCREEN_WIDTH || y >= DG_SCREEN_HEIGHT {
            return;
        }
        let idx = y * DG_SCREEN_WIDTH + x;
        unsafe {
            DOOM_SCREEN_BUFFER[idx] = color_idx;
        }
    }

    pub fn dg_draw_frame() {
        if !unsafe { DG_INITIALIZED } {
            return;
        }
        let buf_ptr = dg_get_screen_buffer() as usize;
        let buf_len = DOOM_BUFFER_SIZE;
        fb_blit(buf_ptr, buf_len);
    }

    pub fn dg_get_ticks_ms() -> u32 {
        tg_get_ticks_ms()
    }

    pub fn dg_sleep_ms(ms: u32) {
        tg_sleep_ms(ms)
    }

    pub fn dg_get_key() -> u32 {
        let c = tg_getchar_poll();
        if c < 0 {
            return 0;
        }
        match c as u8 {
            b'\r' | b' ' => 0x1d,
            b'w' | b'W' => 0xc8,
            b's' | b'S' => 0xd0,
            b'a' | b'A' => 0xcb,
            b'd' | b'D' => 0xcd,
            b'j' | b'J' => 0x21,
            b'q' | b'Q' => 0x01,
            _ => 0,
        }
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

pub fn getchar() -> u8 {
    let mut c = [0u8; 1];
    while read(STDIN, &mut c) != 1 {
        let _ = sched_yield();
    }
    c[0]
}

pub fn get_time() -> isize {
    let mut time: TimeSpec = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    (time.tv_sec * 1000 + time.tv_nsec / 1_000_000) as isize
}

pub fn sleep(period_ms: usize) {
    let mut time: TimeSpec = TimeSpec::ZERO;
    clock_gettime(ClockId::CLOCK_MONOTONIC, &mut time as *mut _ as _);
    let time = time + TimeSpec::from_millsecond(period_ms);
    loop {
        let mut now: TimeSpec = TimeSpec::ZERO;
        clock_gettime(ClockId::CLOCK_MONOTONIC, &mut now as *mut _ as _);
        if now > time {
            break;
        }
        sched_yield();
    }
}

pub fn trace_read(ptr: *const u8) -> Option<u8> {
    let ret = trace(0, ptr as usize, 0);
    if ret >= 0 && ret <= 255 {
        Some(ret as u8)
    } else {
        None
    }
}

pub fn trace_write(ptr: *const u8, value: u8) -> isize {
    trace(1, ptr as usize, value as usize)
}

pub fn count_syscall(syscall_id: usize) -> isize {
    trace(2, syscall_id, 0)
}

/// 从管道读取数据
pub fn pipe_read(pipe_fd: usize, buffer: &mut [u8]) -> isize {
    let mut total_read = 0usize;
    let len = buffer.len();
    loop {
        if total_read >= len {
            return total_read as isize;
        }
        let ret = read(pipe_fd, &mut buffer[total_read..]);
        if ret == -2 {
            sched_yield();
            continue;
        } else if ret == 0 {
            return total_read as isize;
        } else if ret < 0 {
            return ret;
        } else {
            total_read += ret as usize;
        }
    }
}

/// 向管道写入数据
pub fn pipe_write(pipe_fd: usize, buffer: &[u8]) -> isize {
    let mut total_write = 0usize;
    let len = buffer.len();
    loop {
        if total_write >= len {
            return total_write as isize;
        }
        let ret = write(pipe_fd, &buffer[total_write..]);
        if ret == -2 {
            sched_yield();
            continue;
        } else if ret < 0 {
            return ret;
        } else {
            total_write += ret as usize;
        }
    }
}

struct Console;

impl tg_console::Console for Console {
    #[inline]
    fn put_char(&self, c: u8) {
        tg_syscall::write(STDOUT, &[c]);
    }

    #[inline]
    fn put_str(&self, s: &str) {
        tg_syscall::write(STDOUT, s.as_bytes());
    }
}
