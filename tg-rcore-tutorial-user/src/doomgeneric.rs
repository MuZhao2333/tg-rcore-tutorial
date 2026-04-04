//! doomgeneric 移植实现
//!
//! 这是一个简化的 doomgeneric 实现，用于在 rCore-Tutorial-ch8 上运行 DOOM。
//! 实现了 DOOM 游戏所需的显示和输入接口。

use crate::{fb_blit, sleep, getchar, get_time};

/// DOOM 屏幕分辨率
pub const DG_SCREEN_WIDTH: usize = 320;
pub const DG_SCREEN_HEIGHT: usize = 200;

/// 离屏缓冲区大小（320*200 = 64000 bytes，调色板索引）
const DOOM_BUFFER_SIZE: usize = DG_SCREEN_WIDTH * DG_SCREEN_HEIGHT;

/// DOOM 离屏缓冲区（静态分配，存调色板索引）
static mut DOOM_SCREEN_BUFFER: [u8; DOOM_BUFFER_SIZE] = [0u8; DOOM_BUFFER_SIZE];

/// 是否已初始化
static mut DG_INITIALIZED: bool = false;

/// 初始化 doomgeneric
pub fn dg_init() {
    unsafe {
        if DG_INITIALIZED {
            return;
        }
        DG_INITIALIZED = true;
    }
}

/// 获取 DOOM 屏幕缓冲区指针
pub fn dg_get_screen_buffer() -> *mut u8 {
    unsafe { core::ptr::addr_of_mut!(DOOM_SCREEN_BUFFER) as *mut u8 }
}

/// 设置像素（DOOM 游戏调用，将调色板索引写入离屏缓冲区）
pub fn dg_set_pixel(x: usize, y: usize, color_idx: u8) {
    if x >= DG_SCREEN_WIDTH || y >= DG_SCREEN_HEIGHT {
        return;
    }
    let idx = y * DG_SCREEN_WIDTH + x;
    unsafe {
        DOOM_SCREEN_BUFFER[idx] = color_idx;
    }
}

/// 刷新显示：将离屏缓冲区的调色板索引通过内核复制到 GPU 显存
pub fn dg_draw_frame() {
    if !unsafe { DG_INITIALIZED } {
        return;
    }

    let buf_ptr = dg_get_screen_buffer() as usize;
    let buf_len = DOOM_BUFFER_SIZE;

    // 调用内核 syscall：内核完成调色板展开、缩放、复制到 GPU 显存
    fb_blit(buf_ptr, buf_len);
}

/// 获取当前时间（毫秒）
pub fn dg_get_ticks_ms() -> u32 {
    get_time() as u32
}

/// 睡眠（毫秒）
pub fn dg_sleep_ms(ms: u32) {
    sleep(ms as usize);
}

/// 获取按键输入
pub fn dg_get_key() -> u32 {
    let c = getchar();
    if c == 0 {
        return 0;
    }

    match c {
        b'\r' | b' ' => 0x1d,
        b'w' | b'W' => 0xc8,
        b's' | b'S' => 0xd0,
        b'a' | b'A' => 0xcb,
        b'd' | b'D' => 0xcd,
        b'1' => 0x02,
        b'2' => 0x03,
        b'3' => 0x04,
        b'4' => 0x05,
        b'5' => 0x06,
        b'6' => 0x07,
        b'7' => 0x08,
        b'8' => 0x09,
        b'9' => 0x0a,
        b'0' => 0x0b,
        b'f' | b'F' => 0x21,
        b'e' | b'E' => 0x12,
        b'r' | b'R' => 0x13,
        _ => 0,
    }
}
