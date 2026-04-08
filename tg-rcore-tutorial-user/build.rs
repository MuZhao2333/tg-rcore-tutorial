use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LOG");
    println!("cargo:rerun-if-env-changed=BASE_ADDRESS");
    println!("cargo:rerun-if-env-changed=CHAPTER");
    println!("cargo:rerun-if-env-changed=TG_ENABLE_DOOM_C");
    println!("cargo:rerun-if-env-changed=TG_DOOM_FULL");
    println!("cargo:rerun-if-changed=src/bin/doomgeneric");
    println!("cargo:rustc-check-cfg=cfg(tg_doom_c)");

    if let Ok(chapter) = env::var("CHAPTER") {
        println!("cargo:rustc-env=CHAPTER={chapter}");
    }

    // 尝试构建 DOOM C 代码
    try_build_doom_c();

    if let Some(base) = env::var("BASE_ADDRESS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        let text = format!(
            "\
OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {{
    . = {base};
    .text : {{
        *(.text.entry)
        *(.text .text.*)
    }}
    .rodata : {{
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }}
    .data : {{
        *(.data .data.*)
        *(.sdata .sdata.*)
    }}
    .bss : {{
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }}
}}"
        );
        let ld = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("linker.ld");
        fs::write(&ld, text).unwrap();
        println!("cargo:rustc-link-arg=-T{}", ld.display());
    }
}

/// 尝试编译 DOOM C 代码
/// 需要环境变量 TG_ENABLE_DOOM_C=1
fn try_build_doom_c() {
    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("riscv64") {
        return;
    }

    let enable = env::var("TG_ENABLE_DOOM_C")
        .map(|s| s == "1")
        .unwrap_or(false);
    if !enable {
        return;
    }

    let full = env::var("TG_DOOM_FULL")
        .map(|s| s == "1")
        .unwrap_or(false);

    // 查找交叉编译器
    let compiler = find_cross_compiler();
    let Some(compiler) = compiler else {
        println!("cargo:warning=skip doom C build: no cross C compiler found");
        println!("cargo:warning=set CC=riscv64-linux-gnu-gcc or install riscv64-unknown-elf-gcc");
        return;
    };

    println!("cargo:warning=building doom C code with {}", compiler);

    // 配置 cc Build
    let mut build = cc::Build::new();
    build.no_default_flags(true);
    build.compiler(&compiler);
    build.target(&target);
    build.include("src/bin/doomgeneric");

    // RISC-V 编译参数
    build.flag("-march=rv64gc");
    build.flag("-mabi=lp64d");
    build.flag("-O2");
    build.flag("-ffreestanding");
    build.flag("-fno-builtin");
    build.flag("-fno-stack-protector");
    build.flag("-fno-PIC");
    build.warnings(false);

    // 定义分辨率宏
    build.define("DOOMGENERIC_RESX", "320");
    build.define("DOOMGENERIC_RESY", "200");

    if full {
        build.define("TG_DOOM_FULL", None);
        // 添加所有 doomgeneric C 源文件
        for file in DOOM_FULL_SOURCES {
            build.file(format!("src/bin/doomgeneric/{file}"));
        }
    } else {
        // 仅编译平台适配层
        build.file("src/bin/doomgeneric/doomgeneric_tg.c");
    }

    build.compile("doomtg");
    println!("cargo:rustc-cfg=tg_doom_c");
    if full {
        println!("cargo:rustc-cfg=tg_doom_full");
    }
}

/// 查找 RISC-V 交叉编译器
fn find_cross_compiler() -> Option<String> {
    // 1. 优先使用 CC 环境变量
    if let Ok(cc) = env::var("CC") {
        let cc = cc.trim();
        if !cc.is_empty() {
            if command_exists(cc) {
                return Some(cc.to_string());
            }
        }
    }

    // 2. 尝试常见的交叉编译器名称
    let candidates = [
        // WSL Linux 环境
        "riscv64-linux-gnu-gcc",
        "riscv64-unknown-elf-gcc",
        "riscv64-elf-gcc",
        // Windows MSYS2/MinGW
        "riscv64-elf-gcc.exe",
        "riscv64-unknown-elf-gcc.exe",
    ];

    for candidate in &candidates {
        if command_exists(candidate) {
            return Some(candidate.to_string());
        }
    }

    None
}

fn command_exists(cmd: &str) -> bool {
    // 尝试用 sh 命令检测
    let output = Command::new("sh")
        .args(["-c", &format!("command -v {} >/dev/null 2>&1", cmd)])
        .output();
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// DOOM 完整引擎所需的 C 源文件列表
const DOOM_FULL_SOURCES: &[&str] = &[
    // 引擎核心
    "am_map.c",
    "doomdef.c",
    "doomstat.c",
    "dstrings.c",
    "d_event.c",
    "d_items.c",
    "d_iwad.c",
    "d_loop.c",
    "d_main.c",
    "d_mode.c",
    "d_net.c",
    "f_finale.c",
    "f_wipe.c",
    "g_game.c",
    "hu_lib.c",
    "hu_stuff.c",
    "info.c",
    // 输入/计时（跳过需要 SDL/allegro 的）
    // i_sound.c 已由 dummy.c 提供空实现
    "i_cdmus.c",
    "i_endoom.c",
    "i_joystick.c",
    "i_scale.c",
    // i_sound.c 提供声音变量和音乐模块引用
    "i_sound.c",
    "i_system.c",
    "i_timer.c",
    "i_video.c",
    "i_input.c",
    // 工具
    "memio.c",
    "m_argv.c",
    "m_bbox.c",
    "m_cheat.c",
    "m_config.c",
    "m_controls.c",
    "m_fixed.c",
    "m_menu.c",
    "m_misc.c",
    "m_random.c",
    // 地图/游戏逻辑
    "p_ceilng.c",
    "p_doors.c",
    "p_enemy.c",
    "p_floor.c",
    "p_inter.c",
    "p_lights.c",
    "p_map.c",
    "p_maputl.c",
    "p_mobj.c",
    "p_plats.c",
    "p_pspr.c",
    "p_saveg.c",
    "p_setup.c",
    "p_sight.c",
    "p_spec.c",
    "p_switch.c",
    "p_telept.c",
    "p_tick.c",
    "p_user.c",
    // 渲染
    "r_bsp.c",
    "r_data.c",
    "r_draw.c",
    "r_main.c",
    "r_plane.c",
    "r_segs.c",
    "r_sky.c",
    "r_things.c",
    "sha1.c",
    // 音频/界面
    "sounds.c",
    "statdump.c",
    "st_lib.c",
    "st_stuff.c",
    "s_sound.c",
    "tables.c",
    "v_video.c",
    "wi_stuff.c",
    // 文件/资源
    "w_checksum.c",
    "w_file.c",
    "w_file_stdc.c",
    "w_main.c",
    "w_wad.c",
    "z_zone.c",
    // doomgeneric 核心
    "doomgeneric.c",
    // 平台适配层
    "doomgeneric_tg.c",
    // C 标准库替代
    "tg_libc_shim.c",
];
