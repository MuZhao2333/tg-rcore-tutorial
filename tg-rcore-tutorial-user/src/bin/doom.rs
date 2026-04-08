#![no_std]
#![no_main]

//! DOOM 游戏演示程序
//!
//! 支持两种模式：
//! 1. tg_doom_c: 调用真实 DOOM 引擎（C 代码）
//! 2. demo 模式: 显示彩色动画演示

extern crate user_lib;

use user_lib::*;

// ============================================================================
// C 代码绑定（仅在 tg_doom_c 启用时有效）
// ============================================================================

#[cfg(tg_doom_c)]
unsafe extern "C" {
    fn doomgeneric_Create(argc: i32, argv: *mut *mut i8);
    fn doomgeneric_Tick();
}

#[cfg(not(tg_doom_c))]
unsafe extern "C" {
    fn tg_doom_step(ticks_ms: u32, key: i32) -> i32;
}

// ============================================================================
// Demo 模式：彩色动画演示（当 C 引擎未启用时使用）
// ============================================================================

fn fast_sin(x: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let mut n = x % (2.0 * pi);
    if n < 0.0 { n += 2.0 * pi; }
    if n > pi { n -= 2.0 * pi; }
    let x2 = n * n;
    let x3 = x2 * n;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    n - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0
}

fn hsv_to_rgb(h: u16, s: f32, v: f32) -> u8 {
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
    ((r + m) * 255.0) as u8
}

fn demo_main() {
    println!("=== DOOM Demo Mode ===");
    println!("Note: Enable TG_ENABLE_DOOM_C=1 TG_DOOM_FULL=1 for full DOOM");
    println!("Framebuffer: {}x{}", doomgeneric::DG_SCREEN_WIDTH, doomgeneric::DG_SCREEN_HEIGHT);

    doomgeneric::dg_init();

    let mut frame: u32 = 0;
    let max_frames = 300;
    const IDX_RED: u8 = 171;
    const IDX_GREEN: u8 = 176;
    const IDX_BLUE: u8 = 185;
    const IDX_WHITE: u8 = 10;

    while frame < max_frames {
        let t = frame as f32;

        // 绘制渐变背景
        for y in 0..doomgeneric::DG_SCREEN_HEIGHT {
            for x in 0..doomgeneric::DG_SCREEN_WIDTH {
                let idx = ((x / 10) + (y / 10)) % 16;
                doomgeneric::dg_set_pixel(x, y, idx as u8);
            }
        }

        // 绘制 DOOM 标题
        let title = b"DOOM";
        let title_len = title.len();
        let char_scale = 2;
        let char_width = 8 * char_scale;
        let title_total_width = title_len * char_width;
        let title_x = (doomgeneric::DG_SCREEN_WIDTH.saturating_sub(title_total_width)) / 2;
        let title_y = 40;

        for (i, &c) in title.iter().enumerate() {
            let color_idx = match i {
                0 => IDX_RED,
                1 => IDX_GREEN,
                2 => IDX_BLUE,
                _ => IDX_WHITE,
            };
            // 绘制 8x8 字符块
            for sy in 0..(8 * char_scale) {
                for sx in 0..(8 * char_scale) {
                    doomgeneric::dg_set_pixel(title_x + i * char_width + sx, title_y + sy, color_idx);
                }
            }
        }

        // 绘制动画圆形
        let cx = doomgeneric::DG_SCREEN_WIDTH / 2;
        let cy = doomgeneric::DG_SCREEN_HEIGHT / 2 + 20;
        let base_r = 20.0;
        let anim_r = (fast_sin(t * 0.3) * 10.0 + base_r) as i32;
        for dy in -anim_r..anim_r {
            for dx in -anim_r..anim_r {
                if dx * dx + dy * dy <= anim_r * anim_r {
                    let px = (cx as i32 + dx) as usize;
                    let py = (cy as i32 + dy) as usize;
                    if px < doomgeneric::DG_SCREEN_WIDTH && py < doomgeneric::DG_SCREEN_HEIGHT {
                        let s = frame as i64 + dx as i64 + dy as i64;
                        let hue = ((s * 3).rem_euclid(360)) as u16;
                        let gray = hsv_to_rgb(hue, 1.0, 1.0);
                        doomgeneric::dg_set_pixel(px, py, gray);
                    }
                }
            }
        }

        // 绘制进度条
        let bar_x = 20;
        let bar_y = doomgeneric::DG_SCREEN_HEIGHT - 30;
        let bar_w = doomgeneric::DG_SCREEN_WIDTH - 40;
        let bar_h = 8;
        let progress = frame as f32 / max_frames as f32;
        let fill_w = (bar_w as f32 * progress) as usize;

        for y in bar_y..(bar_y + bar_h) {
            for x in bar_x..(bar_x + bar_w) {
                doomgeneric::dg_set_pixel(x, y, IDX_WHITE);
            }
        }
        for y in (bar_y + 1)..(bar_y + bar_h - 1) {
            for x in (bar_x + 1)..(bar_x + fill_w) {
                let hue = (((frame * 2) % 360) as u16);
                let gray = hsv_to_rgb(hue, 1.0, 1.0);
                doomgeneric::dg_set_pixel(x, y, gray);
            }
        }

        doomgeneric::dg_draw_frame();
        sleep(16);
        frame += 1;
    }

    println!("Demo completed {} frames", frame);
}

// ============================================================================
// 主入口
// ============================================================================

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    println!("[DEBUG] doom: starting...");

    #[cfg(tg_doom_c)]
    {
        // 启用 DOOM C 引擎模式
        unsafe {
            println!("[DEBUG] doom: framebuffer check...");
            // 设置轮询输入
            let _ = tg_set_input_mode_polling();

            // 检查 framebuffer
            let mut fb_info = TgFramebufferInfo {
                ptr: core::ptr::null_mut(),
                len: 0,
                width: 0,
                height: 0,
                pitch: 0,
            };
            let fb_ret = tg_framebuffer_info(&mut fb_info);
            println!(
                "[DEBUG] doom: tg_framebuffer_info ret={} {}x{} pitch={} len={}",
                fb_ret, fb_info.width, fb_info.height, fb_info.pitch, fb_info.len
            );

            if fb_ret != 0 || fb_info.width == 0 || fb_info.height == 0 {
                println!("doom: framebuffer unavailable");
                return -1;
            }

            println!("doom: framebuffer {}x{}", fb_info.width, fb_info.height);

            // 检查 WAD 文件
            println!("[DEBUG] doom: checking doom1.wad...");
            let fd = user_lib::open("doom1.wad\0", OpenFlags::RDONLY);
            println!("[DEBUG] doom: open returned fd={}", fd);
            if fd >= 0 {
                println!("doom: found doom1.wad (fd={})", fd);
                let _ = user_lib::close(fd as usize);
            } else {
                println!("doom: doom1.wad not found");
                return -1;
            }

            // 调用 DOOM 引擎
            println!("doom: starting DOOM engine...");
            doomgeneric_Create(1, core::ptr::null_mut());
            println!("doom: doomgeneric_Create returned!");
        }
        0
    }

    #[cfg(not(tg_doom_c))]
    {
        // Demo 模式
        demo_main();
        0
    }
}
