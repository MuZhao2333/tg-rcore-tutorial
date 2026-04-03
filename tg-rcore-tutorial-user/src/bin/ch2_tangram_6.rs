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
    pitch: usize,
    yres: usize,
) {
    let min_y = y1.min(y2).min(y3);
    let max_y = y1.max(y2).max(y3);

    for y in min_y..=max_y {
        let mut x_min = i32::MAX;
        let mut x_max = i32::MIN;

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
                put_pixel(fb, x as usize, y as usize, r, g, b, xres, pitch, yres);
            }
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

    let cy = yres as i32 / 2;
    let rx = xres as i32 * 3 / 4;

    // 程序6：右下平行四边形（橙色）- 用两块三角拼成
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
        pitch,
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
        pitch,
        yres,
    );

    // 右下小三角（品红色）
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
        pitch,
        yres,
    );

    println!("Tangram part 6 (right parallelogram and triangle) rendered");
    println!("All tangram parts rendered successfully!");
    framebuffer_flush();
    0
}
