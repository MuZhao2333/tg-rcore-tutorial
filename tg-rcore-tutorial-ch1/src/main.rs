//! # 第一章：应用程序与基本执行环境 - Tangram 图形显示
//!
//! 本章实现了一个使用 VirtIO GPU 设备显示七巧板（Tangram）图案的裸机程序。

#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

use tg_sbi::{console_putchar, shutdown};
use core::sync::atomic::{AtomicUsize, Ordering};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use virtio_drivers::{Hal, VirtIOGpu, MmioTransport};

/// VirtIO MMIO 基址
const VIRTIO_BASE: usize = 0x1000_0000;

/// 页大小
const PAGE_SIZE: usize = 4096;

/// 显示分辨率
const XRES: usize = 1280;
const YRES: usize = 800;

/// DMA 内存分配计数器
static DMA_ALLOC_IDX: AtomicUsize = AtomicUsize::new(0);

/// DMA 内存基址
const DMA_BASE: usize = 0x8800_0000;

/// 简单全局分配器（实现 GlobalAlloc trait）
struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        // 返回一个指向预分配堆内存的指针
        // 在裸机环境中，我们使用一个固定地址
        0x8400_0000 as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // 不做任何事
    }
}

/// 全局分配器
#[global_allocator]
static GLOBAL: BumpAllocator = BumpAllocator;

/// 简单 HAL 实现
struct SimpleHal;

impl Hal for SimpleHal {
    fn dma_alloc(pages: usize) -> usize {
        let idx = DMA_ALLOC_IDX.fetch_add(pages, Ordering::SeqCst);
        DMA_BASE + idx * PAGE_SIZE
    }

    fn dma_dealloc(_paddr: usize, _pages: usize) -> i32 {
        0
    }

    fn phys_to_virt(paddr: usize) -> usize {
        // 在这个裸机环境中，物理地址就是虚拟地址
        // QEMU virt平台中的MMIO地址已经是正确的虚拟地址
        paddr
    }

    fn virt_to_phys(vaddr: usize) -> usize {
        // 反向映射
        vaddr
    }
}

/// 打印字符
#[inline]
fn put_char(c: u8) {
    console_putchar(c);
}

/// 打印字符串
fn print_str(s: &str) {
    for c in s.as_bytes() {
        put_char(*c);
    }
}

/// 打印十六进制数
fn print_hex(n: usize) {
    const HEXDIGIT: &[u8] = b"0123456789abcdef";
    
    if n == 0 {
        put_char(b'0');
        return;
    }
    
    // 计算数字位数
    let mut temp = n;
    let mut digits = 0;
    while temp > 0 {
        digits += 1;
        temp >>= 4;
    }
    
    // 从高位到低位输出
    for i in (0..digits).rev() {
        let digit = ((n >> (i * 4)) & 0xf) as usize;
        put_char(HEXDIGIT[digit]);
    }
}

/// 打印整数
fn print_int(mut n: u32) {
    if n == 0 {
        put_char(b'0');
        return;
    }
    
    let mut buf = [0u8; 10];
    let mut idx = 0;
    
    while n > 0 {
        buf[idx] = (n % 10) as u8 + b'0';
        n /= 10;
        idx += 1;
    }
    
    for i in (0..idx).rev() {
        put_char(buf[i]);
    }
}

fn draw_tangram(fb: &mut [u8], xres: usize, yres: usize) {
    // 清空屏幕
    for pixel in fb.iter_mut() {
        *pixel = 0;
    }

    // 动态单位，根据较短边划分为若干个unit
    let min_side = xres.min(yres) as i32;
    let u = (min_side / 6).max(20) as i32;

    let lx = xres as i32 / 5; // left cluster center x (move a bit left)
    let rx = xres as i32 * 3 / 4; // right cluster center x
    let cy = yres as i32 / 2; // common center y

    // ------------------
    // 左侧图案（近似 "O" 样式的七巧板）
    // ------------------
    // 红色：左上小三角
    fill_triangle(
        fb,
        lx - 3 * u,
        cy - 3 * u,
        lx - 1 * u,
        cy - 3 * u,
        lx - 3 * u,
        cy - 1 * u,
        180,
        30,
        30,
        xres,
        yres,
    );

    // 黄色：左侧长条（用矩形近似平行四边形）
    fill_rect(fb, lx - 2 * u, cy - 2 * u, u, 4 * u, 255, 200, 30, xres, yres);
    // 补一块小三角用于形状过渡
    fill_triangle(fb, lx - 2 * u, cy - 2 * u, lx - 1 * u, cy - 2 * u, lx - 2 * u, cy - 1 * u, 255, 200, 30, xres, yres);

    // 青色：左下大三角
    fill_triangle(
        fb,
        lx - 2 * u,
        cy + 2 * u,
        lx + 1 * u,
        cy + 2 * u,
        lx - 2 * u,
        cy - 1 * u,
        0,
        200,
        255,
        xres,
        yres,
    );

    // 绿色：左下角正方形（微调位置）
    fill_rect(fb, lx - u / 4, cy + u / 2, u, u, 120, 255, 80, xres, yres);

    // 蓝色：右侧竖直三角
    fill_triangle(
        fb,
        lx + 1 * u,
        cy - 2 * u,
        lx + 1 * u,
        cy + 3 * u,
        lx + 2 * u,
        cy + 1 * u,
        60,
        60,
        255,
        xres,
        yres,
    );

    // 品红：右上大三角
    fill_triangle(
        fb,
        lx + 2 * u,
        cy - 3 * u,
        lx + 4 * u,
        cy - 3 * u,
        lx + 2 * u,
        cy + 1 * u,
        255,
        80,
        200,
        xres,
        yres,
    );

    // ------------------
    // 右侧图案（近似 "S" 样式的七巧板组合）
    // ------------------
    // 青色：右上偏左大三角
    fill_triangle(
        fb,
        rx - 3 * u,
        cy - 2 * u,
        rx - 1 * u,
        cy - 4 * u,
        rx + 0 * u,
        cy - 1 * u,
        0,
        200,
        255,
        xres,
        yres,
    );

    // 蓝色：右上小三角
    fill_triangle(
        fb,
        rx - 1 * u,
        cy - 2 * u,
        rx + 1 * u,
        cy - 1 * u,
        rx - 2 * u,
        cy - 1 * u,
        60,
        60,
        255,
        xres,
        yres,
    );

    // 品红：右上角小三角
    fill_triangle(
        fb,
        rx + 1 * u,
        cy - 2 * u,
        rx + 2 * u,
        cy - 1 * u,
        rx + 0 * u,
        cy - 1 * u,
        255,
        80,
        200,
        xres,
        yres,
    );

    // 绿色：右侧中心正方形
    fill_rect(fb, rx - u / 2, cy - u / 2, u, u, 120, 255, 80, xres, yres);

    // 橙色：右下平行四边形（用两块三角拼成）
    fill_triangle(
        fb,
        rx - 3 * u,
        cy + 1 * u,
        rx - 1 * u,
        cy + 1 * u,
        rx - 2 * u,
        cy + 3 * u,
        255,
        150,
        0,
        xres,
        yres,
    );
    fill_triangle(
        fb,
        rx - 2 * u,
        cy + 1 * u,
        rx + 0 * u,
        cy + 1 * u,
        rx - 1 * u,
        cy + 3 * u,
        255,
        150,
        0,
        xres,
        yres,
    );

    // 品红：右下小三角
    fill_triangle(
        fb,
        rx + 0 * u,
        cy + 1 * u,
        rx + 2 * u,
        cy + 3 * u,
        rx + 0 * u,
        cy + 3 * u,
        255,
        80,
        200,
        xres,
        yres,
    );
}

/// 在 framebuffer 上绘制像素（使用运行时分辨率）
#[inline]
fn put_pixel(
    fb: &mut [u8],
    x: usize,
    y: usize,
    r: u8,
    g: u8,
    b: u8,
    xres: usize,
    yres: usize,
) {
    if x >= xres || y >= yres {
        return;
    }
    let idx = (y * xres + x) * 4;
    if idx + 3 < fb.len() {
        fb[idx] = b;
        fb[idx + 1] = g;
        fb[idx + 2] = r;
        fb[idx + 3] = 0xFF;
    }
}

/// 填充三角形
fn fill_triangle(
    fb: &mut [u8],
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
    r: u8,
    g: u8,
    b: u8,
    xres: usize,
    yres: usize,
) {
    let min_y = y1.min(y2).min(y3);
    let max_y = y1.max(y2).max(y3);

    for y in min_y..=max_y {
        let mut x_min = i32::MAX;
        let mut x_max = i32::MIN;

        // 对三条边进行扫描
        for (xa, ya, xb, yb) in &[(x1, y1, x2, y2), (x2, y2, x3, y3), (x3, y3, x1, y1)] {
            if (*ya <= y && y <= *yb) || (*yb <= y && y <= *ya) {
                if *ya != *yb {
                    let x = *xa + (*xb - *xa) * (y - *ya) / (*yb - *ya);
                    x_min = x_min.min(x);
                    x_max = x_max.max(x);
                }
            }
        }

        if x_min <= x_max {
            for x in x_min..=x_max {
                put_pixel(fb, x as usize, y as usize, r, g, b, xres, yres);
            }
        }
    }
}

/// 填充矩形
fn fill_rect(
    fb: &mut [u8],
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    r: u8,
    g: u8,
    b: u8,
    xres: usize,
    yres: usize,
) {
    for dy in 0..h {
        for dx in 0..w {
            put_pixel(fb, (x + dx) as usize, (y + dy) as usize, r, g, b, xres, yres);
        }
    }
}



/// S 态程序入口点
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 4096;

    #[unsafe(link_section = ".bss.uninit")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        main       =   sym rust_main,
    )
}

/// 主函数：初始化 GPU 并绘制七巧板
extern "C" fn rust_main() -> ! {
    print_str("=== Tangram Graphics System ===\n");
    print_str("Probing VirtIO GPU device...\n");

    // 扫描更广泛的地址范围来查找VirtIO设备
    // QEMU virt平台中MMIO设备从0x10000000开始，每个设备占用0x1000
    let base = VIRTIO_BASE;
    
    for i in 0..16 {
        let addr = base + i * 0x1000;
        
        print_str("Checking 0x");
        print_hex(addr);
        print_str(": ");

        // 尝试读取magic值以检查设备是否存在
        unsafe {
            let magic_ptr = addr as *const u32;
            let magic = *magic_ptr;
            
            if magic != 0 {
                print_str("Found device! Magic=0x");
                print_hex(magic as usize);
                print_str("\n");
                
                // 尝试读取设备类型
                let device_type_ptr = (addr + 8) as *const u32;
                let device_id = *device_type_ptr;
                print_str("Device ID: ");
                print_int(device_id);
                print_str("\n");
                
                // 如果是GPU (ID 16)，尝试初始化
                if device_id == 16 {
                    print_str("Found GPU device!\n");
                    
                    let header_ptr = match NonNull::new(addr as *mut virtio_drivers::VirtIOHeader) {
                        Some(p) => p,
                        None => {
                            print_str("Failed to create pointer\n");
                            continue;
                        }
                    };
                    
                    // 尝试构建transport
                    match MmioTransport::new(header_ptr) {
                        Ok(transport) => {
                            print_str("Transport created!\n");
                            match VirtIOGpu::<SimpleHal, MmioTransport>::new(transport) {
                                Ok(mut gpu) => {
                                    print_str("GPU initialized!\n");
                                    
                                    // 获取设备分辨率并用于绘图
                                    match gpu.resolution() {
                                        Ok((w, h)) => {
                                            let xres = w as usize;
                                            let yres = h as usize;
                                            // Scope for framebuffer operations
                                            {
                                                match gpu.setup_framebuffer() {
                                                    Ok(fb) => {
                                                        print_str("Framebuffer setup! Size: ");
                                                        print_int(fb.len() as u32);
                                                        print_str("\n");

                                                        // 第一步：清空framebuffer
                                                        for pixel in fb.iter_mut() {
                                                            *pixel = 0;
                                                        }

                                                        print_str("Drawing tangram...\n");
                                                        draw_tangram(fb, xres, yres);
                                                    }
                                                    Err(_) => print_str("Framebuffer error\n"),
                                                }
                                            } // fb reference is dropped here
                                        }
                                        Err(_) => {
                                            print_str("Failed to get resolution\n");
                                            // 退回到原来的处理，尽量尝试设置 framebuffer
                                            match gpu.setup_framebuffer() {
                                                Ok(fb) => {
                                                    for pixel in fb.iter_mut() { *pixel = 0; }
                                                    print_str("Drawing tangram...\n");
                                                    // 仍然使用默认常量作为后备
                                                    draw_tangram(fb, XRES, YRES);
                                                }
                                                Err(_) => print_str("Framebuffer error\n"),
                                            }
                                        }
                                    }
                                    
                                    // 长延迟确保drawing完成
                                    for _ in 0..100000 {
                                        core::hint::spin_loop();
                                    }
                                    
                                    print_str("Flushing display...\n");
                                    
                                    // 执行刷新
                                    match gpu.flush() {
                                        Ok(()) => print_str("Flush OK\n"),
                                        Err(_) => print_str("Flush error\n"),
                                    }
                                    
                                    // 最后的极长延迟，让GPU彻底完成所有操作
                                    for _ in 0..200000 {
                                        core::hint::spin_loop();
                                    }
                                    
                                    print_str("Display ready!\n");
                                    
                                    loop {
                                        core::hint::spin_loop();
                                    }
                                }
                                Err(_) => print_str("GPU init error\n"),
                            }
                        }
                        Err(_) => print_str("Transport error\n"),
                    }
                }
            } else {
                print_str("Empty\n");
            }
        }
    }

    print_str("No GPU found!\n");
    shutdown(true);
}

/// panic 处理函数
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    shutdown(true)
}

/// 非 RISC-V64 占位模块
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
