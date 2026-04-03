#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::*;

/// 绘制像素到framebuffer（按真实 pitch 写入）
#[inline]
fn put_pixel(
    fb: &mut [u8],
    x: usize,
    y: usize,
    r: u8,
    g: u8,
    b: u8,
    xres: usize,
    pitch: usize,
    yres: usize,
) {
    if x >= xres || y >= yres {
        return;
    }
    let idx = y * pitch + x * 4;
    if idx + 3 < fb.len() {
        fb[idx] = b;
        fb[idx + 1] = g;
        fb[idx + 2] = r;
        fb[idx + 3] = 0xFF;
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
    pitch: usize,
    yres: usize,
) {
    for dy in 0..h {
        for dx in 0..w {
            put_pixel(
                fb,
                (x + dx) as usize,
                (y + dy) as usize,
                r,
                g,
                b,
                xres,
                pitch,
                yres,
            );
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    let mut fb_info: FbInfo = FbInfo {
        ptr: 0,
        width: 0,
        height: 0,
        pitch: 0,
        format: 0,
    };

    let ret = get_fb_info(&mut fb_info);
    if ret != 0 {
        println!("Failed to get framebuffer info");
        return 1;
    }

    let xres = fb_info.width as usize;
    let yres = fb_info.height as usize;
    let pitch = fb_info.pitch as usize;
    let fb_len = pitch * yres;

    let fb = unsafe { core::slice::from_raw_parts_mut(fb_info.ptr as *mut u8, fb_len) };

    // 动态单位
    let min_side = (xres.min(yres) as i32) / 6;
    let u = min_side.max(20);

    let lx = xres as i32 / 5;
    let cy = yres as i32 / 2;

    // 程序1：左侧长条（黄色）
    fill_rect(fb, lx - 2*u, cy - 2*u, u, 4*u, 255, 200, 30, xres, pitch, yres);

    println!("Tangram part 1 (left bar) rendered");
    framebuffer_flush();
    0
}
