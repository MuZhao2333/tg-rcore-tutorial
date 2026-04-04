#![no_std]
#![no_main]

//! DOOM 游戏演示程序
//!
//! 这是一个 DOOM 游戏移植的演示程序，使用 doomgeneric 接口。

extern crate user_lib;
extern crate alloc;

use user_lib::*;

/// 简单的 sin 函数近似（Taylor 级数展开的前几项）
fn fast_sin(x: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let mut normalized = x % (2.0 * pi);
    if normalized < 0.0 {
        normalized += 2.0 * pi;
    }
    if normalized > pi {
        normalized -= 2.0 * pi;
    }
    let x2 = normalized * normalized;
    let x3 = x2 * normalized;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    normalized - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0
}

/// HSV 到 RGB 转换
fn hsv_to_rgb(h: u16, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h as f32;
    let c = v * s;
    let x_val = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x_val, 0.0)
    } else if h < 120.0 {
        (x_val, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x_val)
    } else if h < 240.0 {
        (0.0, x_val, c)
    } else if h < 300.0 {
        (x_val, 0.0, c)
    } else {
        (c, 0.0, x_val)
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// 设置 DOOM 缓冲区中的像素（320x200 调色板索引 -> BGRA）
fn set_pixel(x: usize, y: usize, color_idx: u8) {
    if x < DG_SCREEN_WIDTH && y < DG_SCREEN_HEIGHT {
        dg_set_pixel(x, y, color_idx);
    }
}

/// 在 DOOM 缓冲区中绘制 8x8 字符块
fn draw_char_block(x: usize, y: usize, block_color_idx: u8, scale: usize) {
    for sy in 0..(8 * scale) {
        for sx in 0..(8 * scale) {
            set_pixel(x + sx, y + sy, block_color_idx);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    println!("=== DOOM rCore-Tutorial Port ===");
    println!("Initializing display...");

    // 初始化 doomgeneric（获取 GPU 分辨率信息）
    dg_init();

    let mut fb_info: FbInfo = FbInfo {
        ptr: 0,
        width: 0,
        height: 0,
        pitch: 0,
        format: 0,
    };

    let ret = get_fb_info(&mut fb_info);
    if ret != 0 || fb_info.width == 0 || fb_info.height == 0 {
        println!("Failed to get framebuffer info, ret={}", ret);
        println!("DOOM requires GPU support. Please run with -vga virtio");
        return 1;
    }

    println!("Framebuffer: {}x{}", fb_info.width, fb_info.height);
    println!("DOOM screen: {}x{}", DG_SCREEN_WIDTH, DG_SCREEN_HEIGHT);
    println!();
    println!("Press any key to start DOOM demo...");
    println!("Note: Full DOOM requires doom1.wad file");

    let _ = getchar();
    println!("Starting DOOM main loop...");

    // 演示模式：显示彩色动画（320x200 分辨率）
    let mut frame: u32 = 0;
    let max_frames = 300;

    // 预计算 DOOM 调色板中一些有用颜色的索引
    const IDX_BLACK: u8 = 0;
    const IDX_DARKGRAY: u8 = 5;
    const IDX_WHITE: u8 = 10;
    const IDX_RED: u8 = 171;
    const IDX_GREEN: u8 = 176;
    const IDX_BLUE: u8 = 185;
    const IDX_YELLOW: u8 = 172;

    while frame < max_frames {
        let t = frame as f32;

        // 绘制渐变背景（320x200 调色板索引）
        for y in 0..DG_SCREEN_HEIGHT {
            for x in 0..DG_SCREEN_WIDTH {
                // 简单的调色板索引映射：基于 x,y 位置选择颜色
                let idx = ((x / 10) + (y / 10)) % 16;
                let color_idx = match idx {
                    0 => IDX_BLACK,
                    1 => IDX_DARKGRAY,
                    2 => IDX_WHITE,
                    3 => IDX_RED,
                    4 => IDX_GREEN,
                    5 => IDX_BLUE,
                    6 => IDX_YELLOW,
                    _ => idx as u8,
                };
                set_pixel(x, y, color_idx);
            }
        }

        // 绘制 DOOM 风格的标题 "DOOM"
        let title = b"DOOM";
        let title_len = title.len();
        let char_block = 8;
        let char_scale = 2;
        let char_width = char_block * char_scale;
        let char_height = char_block * char_scale;
        let title_total_width = title_len * char_width;
        let title_x = (DG_SCREEN_WIDTH.saturating_sub(title_total_width)) / 2;
        let title_y = 40;

        for (i, &c) in title.iter().enumerate() {
            // 每个字符用不同的颜色
            let color_idx = match i {
                0 => IDX_RED,
                1 => IDX_GREEN,
                2 => IDX_BLUE,
                3 => IDX_YELLOW,
                _ => IDX_WHITE,
            };
            draw_char_block(
                title_x + i * char_width,
                title_y,
                color_idx,
                char_scale,
            );
        }

        // 绘制 "rCore-Tutorial" 子标题
        let subtitle = b"rCore Ch8";
        let sub_len = subtitle.len();
        let sub_width = 4 * sub_len; // 小字符
        let sub_x = (DG_SCREEN_WIDTH.saturating_sub(sub_width)) / 2;
        let sub_y = title_y + char_height + 4;
        for (i, &_) in subtitle.iter().enumerate() {
            draw_char_block(sub_x + i * 4, sub_y, IDX_WHITE, 1);
        }

        // 绘制动画圆形
        let cx = DG_SCREEN_WIDTH / 2;
        let cy = DG_SCREEN_HEIGHT / 2 + 20;
        let base_r = 20.0;
        let anim_r = (fast_sin(t * 0.3) * 10.0 + base_r) as i32;
        for dy in -anim_r..anim_r {
            for dx in -anim_r..anim_r {
                if dx * dx + dy * dy <= anim_r * anim_r {
                    let px = (cx as i32 + dx) as usize;
                    let py = (cy as i32 + dy) as usize;
                    if px < DG_SCREEN_WIDTH && py < DG_SCREEN_HEIGHT {
                        // 彩虹颜色（dx+dy 可能为负，禁止 (负数 as u32) 再乘，否则会 u32 溢出）
                        let s = frame as i64 + dx as i64 + dy as i64;
                        let hue = ((s * 3).rem_euclid(360)) as u16;
                        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                        let gray = ((r as u32 + g as u32 + b as u32) / 3) as u8;
                        set_pixel(px, py, gray);
                    }
                }
            }
        }

        // 绘制进度条
        let bar_x = 20;
        let bar_y = DG_SCREEN_HEIGHT - 30;
        let bar_w = DG_SCREEN_WIDTH - 40;
        let bar_h = 8;
        let progress = frame as f32 / max_frames as f32;
        let fill_w = (bar_w as f32 * progress) as usize;

        // 进度条边框
        for y in bar_y..(bar_y + bar_h) {
            for x in bar_x..(bar_x + bar_w) {
                set_pixel(x, y, IDX_WHITE);
            }
        }
        // 进度条填充
        for y in (bar_y + 1)..(bar_y + bar_h - 1) {
            for x in (bar_x + 1)..(bar_x + fill_w) {
                // 彩虹进度
                let hue = (((frame as u32 * 2) % 360) as u16);
                let (r, _, b) = hsv_to_rgb(hue, 1.0, 1.0);
                let gray = ((r as u32 + b as u32) / 2) as u8;
                set_pixel(x, y, gray);
            }
        }

        // 调用 dg_draw_frame()：内核完成调色板展开+缩放+复制到 GPU
        dg_draw_frame();

        sleep(16);
        frame += 1;
    }

    println!("Demo completed {} frames", frame);
    0
}
