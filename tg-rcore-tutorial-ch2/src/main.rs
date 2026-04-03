//! # 第二章：批处理系统
//!
//! 本章在第一章"最小执行环境"的基础上，实现了一个**批处理操作系统**，
//! 能够依次加载并运行多个用户程序。
//!
//! ## 核心概念
//!
//! - **批处理系统**：将多个用户程序打包，自动依次执行
//! - **特权级切换**：U-mode（用户态）与 S-mode（内核态）之间的切换
//! - **Trap 处理**：用户程序通过 `ecall` 触发系统调用，或因异常陷入内核
//! - **上下文保存与恢复**：进入/退出 Trap 时保存/恢复用户寄存器状态
//! - **系统调用**：`write`（输出）和 `exit`（退出）
//!
//! 教程阅读建议：
//!
//! - 先看 `rust_main` 的 for-loop：理解批处理“逐个装载、逐个执行”；
//! - 再看 `handle_syscall`：理解 a7/a0-a5/a0 的系统调用寄存器约定；
//! - 最后看 `impls`：把内核态 trait 接口与用户态 syscall 行为对齐起来。

// 不使用标准库，裸机环境没有操作系统提供系统调用支持
#![no_std]
// 不使用标准入口，裸机环境没有 C runtime 进行初始化
#![no_main]
// RISC-V64 架构下启用严格警告和文档检查
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
// 非 RISC-V64 架构允许死代码（用于 cargo publish --dry-run 在主机上通过编译）
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

// 引入控制台输出宏（print! / println!），由 tg_console 库提供
#[macro_use]
extern crate tg_console;

// 本地模块：Console 和 SyscallContext 的实现、GPU 驱动
mod gpu;

use impls::{Console, SyscallContext};
// riscv 库：访问 RISC-V 控制状态寄存器（CSR），如 scause
use riscv::register::*;
// 日志模块
use tg_console::log;
// 用户上下文：保存/恢复用户态寄存器，实现特权级切换
use tg_kernel_context::LocalContext;
// SBI 调用：关机等
use tg_sbi;
// 系统调用相关：调用者信息、系统调用 ID
use tg_syscall::{Caller, SyscallId};

// ========== 全局分配器 ==========

#[global_allocator]
static GLOBAL: gpu::BumpAllocator = gpu::BumpAllocator;

// ========== 启动相关 ==========

// 将用户程序的二进制数据内联到内核镜像的 .data 段中
// APP_ASM 由 build.rs 在编译时生成，包含所有用户程序的二进制数据
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(include_str!(env!("APP_ASM")));

// 定义内核入口点：设置 8 页（32 KiB）的内核栈，然后跳转到 rust_main。
//
// 这里不再调用 tg_linker::boot0! 宏，避免外部已发布版本与 Rust 2024
// 在属性语义上的兼容差异影响本 crate 的发布校验。
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 8 * 4096;
    #[unsafe(link_section = ".boot.stack")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack = sym STACK,
        stack_size = const STACK_SIZE,
        main = sym rust_main,
    )
}

// ========== 内核主函数 ==========

/// 内核主函数：初始化各子系统，然后以批处理方式依次运行所有用户程序。
extern "C" fn rust_main() -> ! {
    // 第一步：清零 BSS 段（未初始化的全局变量区域）
    unsafe { tg_linker::KernelLayout::locate().zero_bss() };

    // 第二步：初始化控制台输出（使 print!/println! 可用）
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    tg_console::test_log();

    // 第三步：初始化 GPU 设备
    log::info!("Initializing GPU device...");
    let (fb_ptr, fb_len, fb_width, fb_height) = gpu::init_gpu();
    log::info!("Framebuffer: ptr=0x{:x}, len={}, {}x{}", fb_ptr, fb_len, fb_width, fb_height);

    // 第四步：初始化系统调用处理（注册 IO、Process 和 Display 的实现）
    log::info!("Initializing syscalls...");
    tg_syscall::init_io(&SyscallContext);
    log::info!("IO syscalls initialized");
    tg_syscall::init_process(&SyscallContext);
    log::info!("Process syscalls initialized");
    tg_syscall::init_display(&SyscallContext);
    log::info!("Display syscalls initialized");

    // 第五步：批处理——依次加载并运行每个用户程序
    log::info!("Starting batch processing...");
    println!("\n✓✓✓ BATCH START ✓✓✓\n");

    // 进一步的运行时诊断：保存 meta 指针以便在循环内比对原始位置与拷贝后的地址/数据
    let diag_meta = tg_linker::AppMeta::locate();
    let diag_p64 = diag_meta as *const _ as *const u64;
    let diag_base = unsafe { *diag_p64.add(0) } as usize;
    let diag_step = unsafe { *diag_p64.add(1) } as usize;
    let diag_count = unsafe { *diag_p64.add(2) } as usize;
    let diag_entries = unsafe { diag_p64.add(3) as *const usize };

    for i in 0..diag_count {
        println!("\n>>> APP{} START <<<", i);
        log::info!("[ITER] begin iteration i={}", i);

        // 原始嵌入位置与大小（由链接器 app table 指定）
        let orig_pos = unsafe { *diag_entries.add(i) };
        let orig_next = unsafe { *diag_entries.add(i + 1) };
        let orig_size = orig_next.wrapping_sub(orig_pos);
        let expected_base = diag_base.wrapping_add(i.wrapping_mul(diag_step));

        // 如果 meta 指定了 base，则将应用拷贝到 base + i*step；否则直接使用嵌入位置
        let app_base = if diag_base != 0 {
            let dest = expected_base;
            log::info!(
                "[COPY] about to copy orig_pos=0x{:x} -> dest=0x{:x}, size={}",
                orig_pos,
                dest,
                orig_size
            );
            unsafe {
                // 分块拷贝，便于在长拷贝中打印进度定位挂起点
                let mut remaining = orig_size;
                let mut src = orig_pos as *const u8;
                let mut dst_ptr = dest as *mut u8;
                const CHUNK: usize = 4096;
                let mut copied: usize = 0;
                while remaining > 0 {
                    let c = core::cmp::min(CHUNK, remaining);
                    core::ptr::copy_nonoverlapping(src, dst_ptr, c);
                    remaining -= c;
                    copied += c;
                    src = src.add(c);
                    dst_ptr = dst_ptr.add(c);
                    // 只打印第一次和每 64 KiB 的进度，避免日志过多
                    if copied == c || (copied % 65536) == 0 {
                        log::info!("[COPY] progress i={} copied {} bytes", i, copied);
                    }
                }

                // 仅清零 slot 的前 4 KiB 作为占位，完整清零会写入大量内存，暂时避免
                if diag_step > orig_size {
                    let z = core::cmp::min(4096, diag_step - orig_size);
                    core::slice::from_raw_parts_mut((dest + orig_size) as *mut u8, z).fill(0);
                }
            }
            log::info!("[COPY] done copying to 0x{:x}", dest);
            dest
        } else {
            orig_pos
        };

        log::info!("load app{} to {:#x}, size={}", i, app_base, orig_size);
        log::info!("[DIAG2] app{}: orig_pos=0x{:x}, orig_size={}, expected_base=0x{:x}", i, orig_pos, orig_size, expected_base);

        // 读取首若干字节并打印十六进制便于比较（最多 8 字节）
        let first_n = core::cmp::min(8, orig_size);
        let mut orig_word: u64 = 0;
        let mut loaded_word: u64 = 0;
        for j in 0..first_n {
            let b = unsafe { *((orig_pos as *const u8).add(j)) } as u64;
            orig_word |= b << (j * 8);
            let lb = unsafe { *((app_base as *const u8).add(j)) } as u64;
            loaded_word |= lb << (j * 8);
        }
        log::info!("[DIAG2] app{} first{}bytes: orig=0x{:x}, loaded=0x{:x}", i, first_n, orig_word, loaded_word);

        // 创建用户态上下文，入口地址为 app_base
        let mut ctx = LocalContext::user(app_base);

        // 分配用户栈（4 KiB），使用 MaybeUninit 避免不必要的零初始化
        let mut user_stack: core::mem::MaybeUninit<[usize; 512]> = core::mem::MaybeUninit::uninit();
        let user_stack_ptr = user_stack.as_mut_ptr() as *mut usize;
        *ctx.sp_mut() = unsafe { user_stack_ptr.add(512) } as usize;
        log::info!("[CTX] sp set to 0x{:x}, entry=0x{:x}", *ctx.sp_mut(), app_base);

        // 循环执行用户程序，直到退出或出错
        loop {
            log::info!("[EXEC] About to execute user program, pc=0x{:x}", ctx.pc());
            unsafe { ctx.execute() };

            use scause::{Exception, Trap};
            let cause = scause::read().cause();
            log::debug!("[EXEC] Trap occurred: {:?}, pc=0x{:x}", cause, ctx.pc());
            match cause {
                Trap::Exception(Exception::UserEnvCall) => {
                    use SyscallResult::*;
                    log::debug!("[EXEC] UserEnvCall: a7={}", ctx.a(7));
                    match handle_syscall(&mut ctx) {
                        Done => {
                            log::debug!("[EXEC] Syscall handled, continuing");
                            continue;
                        }
                        Exit(code) => {
                            log::info!("[ INFO] app{} exit with code {}", i, code);
                        }
                        Error(id) => {
                            log::error!("[EXEC] app{} call an unsupported syscall {}", i, id.0)
                        }
                    }
                }
                trap => log::error!("[EXEC] app{} was killed because of {:?}", i, trap),
            }
            unsafe { core::arch::asm!("fence.i") };
            break;
        }
        // 防止编译器优化掉 user_stack
        let _ = core::hint::black_box(&user_stack);

        // 运行结束后再次打印 AppMeta 与表项，检查是否被覆盖或改变
        unsafe {
            let m2 = tg_linker::AppMeta::locate();
            let p2 = m2 as *const _ as *const u64;
            let b2 = *p2 as usize;
            let s2 = *p2.add(1) as usize;
            let c2 = *p2.add(2) as usize;
            log::info!("[DIAG_AFTER] AppMeta: base=0x{:x}, step=0x{:x}, count={}", b2, s2, c2);
            let ent2 = p2.add(3) as *const usize;
            for k in 0..(c2 + 1) {
                let v2 = *ent2.add(k);
                log::info!("[DIAG_AFTER] app_table[{}] = 0x{:x}", k, v2);
            }
        }

        println!("<<< APP{} END >>>", i);
        println!();
    }

    println!("\n✓✓✓ BATCH END ✓✓✓\n");

    // 所有用户程序执行完毕 —— 进入调试空循环以保留 framebuffer 便于观察（临时调试用）。
    log::info!("All apps finished — entering debug idle loop (no shutdown).");
    loop {
        // 在 RISC-V 真机/仿真上，尝试周期性刷新 GPU 并执行低功耗等待（wfi），
        // 以便 SDL/虚拟 GPU 能够及时显示 framebuffer 内容。
        #[cfg(target_arch = "riscv64")]
        {
            let _ = gpu::gpu_flush();
            unsafe { core::arch::asm!("wfi"); }
        }
    }
}

// ========== panic 处理 ==========

/// panic 处理函数：打印错误信息后以异常状态关机。
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");
    tg_sbi::shutdown(true)
}

// ========== 系统调用处理 ==========

/// 系统调用处理结果
enum SyscallResult {
    /// 系统调用完成，继续执行用户程序
    Done,
    /// 用户程序请求退出，附带退出码
    Exit(usize),
    /// 不支持的系统调用
    Error(SyscallId),
}

/// 处理系统调用。
///
/// 从用户上下文中提取系统调用 ID（a7 寄存器）和参数（a0-a5 寄存器），
/// 分发到对应的处理函数，并将返回值写回 a0 寄存器。
fn handle_syscall(ctx: &mut LocalContext) -> SyscallResult {
    use tg_syscall::{SyscallId as Id, SyscallResult as Ret};

    // a7 寄存器存放 syscall ID
    let id = ctx.a(7).into();
    // a0-a5 寄存器存放系统调用参数
    let args = [ctx.a(0), ctx.a(1), ctx.a(2), ctx.a(3), ctx.a(4), ctx.a(5)];

    tg_console::log::debug!("[SYSCALL] Handling syscall ID={}, args={:?}", ctx.a(7), args);

    match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
        Ret::Done(ret) => match id {
            Id::EXIT => SyscallResult::Exit(ctx.a(0)),
            _ => {
                // 将返回值写入 a0
                *ctx.a_mut(0) = ret as _;
                // sepc += 4，使 sret 后从 ecall 的下一条指令继续执行
                ctx.move_next();
                SyscallResult::Done
            }
        },
        Ret::Unsupported(id) => SyscallResult::Error(id),
    }
}

// ========== 接口实现 ==========

/// 各依赖库所需接口的具体实现
mod impls {
    use tg_syscall::{STDDEBUG, STDOUT};

    /// 控制台实现：通过 SBI 逐字符输出
    pub struct Console;

    impl tg_console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) {
            tg_sbi::console_putchar(c);
        }
    }

    /// 系统调用上下文实现：处理 IO 和 Process 相关的系统调用
    pub struct SyscallContext;

    /// IO 系统调用实现：处理 write 系统调用
    impl tg_syscall::IO for SyscallContext {
        fn write(
            &self,
            _caller: tg_syscall::Caller,
            fd: usize,
            buf: usize,
            count: usize,
        ) -> isize {
            match fd {
                // 标准输出和调试输出：将缓冲区内容打印到控制台
                STDOUT | STDDEBUG => {
                    print!("{}", unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                            buf as *const u8,
                            count,
                        ))
                    });
                    count as _
                }
                _ => {
                    tg_console::log::error!("unsupported fd: {fd}");
                    -1
                }
            }
        }
    }

    /// Process 系统调用实现：处理 exit 系统调用
    impl tg_syscall::Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: tg_syscall::Caller, _status: usize) -> isize {
            0
        }
    }

    /// Display 系统调用实现：处理 get_fb_info 和 framebuffer_flush 系统调用
    impl tg_syscall::Display for SyscallContext {
        fn get_fb_info(
            &self,
            _caller: tg_syscall::Caller,
            info_ptr: usize,
        ) -> isize {
            use tg_syscall::FbInfo;
            tg_console::log::debug!("[SYSCALL] get_fb_info() called, info_ptr=0x{:x}", info_ptr);
            let (width, height) = super::gpu::get_resolution();
            let fb_ptr = super::gpu::get_framebuffer_ptr();

            if fb_ptr == 0 {
                tg_console::log::error!("[SYSCALL] Framebuffer not initialized");
                return -1;
            }

            // 尝试获取真实的 pitch（字节/行），若不可用则回退为 width*4
            let pitch_raw = super::gpu::get_framebuffer_pitch();
            let pitch = if pitch_raw > 0 { pitch_raw as u32 } else { width * 4 };

            tg_console::log::debug!("[SYSCALL] Returning framebuffer: ptr=0x{:x}, {}x{}, pitch={}", fb_ptr, width, height, pitch);
            
            let info = info_ptr as *mut FbInfo;
            unsafe {
                (*info).ptr = fb_ptr;
                (*info).width = width;
                (*info).height = height;
                (*info).pitch = pitch; // bytes per line
                (*info).format = 0; // BGR888
            }

            0  // 返回成功
        }

        fn framebuffer_flush(&self, _caller: tg_syscall::Caller) -> isize {
            tg_console::log::debug!("[SYSCALL] framebuffer_flush() called");
            // 调用GPU驱动刷新显示
            let result = super::gpu::gpu_flush();
            tg_console::log::debug!("[SYSCALL] framebuffer_flush() returned {}", result);
            result
        }
    }
}

/// 非 RISC-V64 架构的占位模块。
///
/// 提供编译所需的符号，使得 `cargo publish --dry-run` 在主机平台上能通过编译。
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    /// 主机平台占位入口
    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 {
        0
    }

    /// C 运行时占位
    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 {
        0
    }

    /// Rust 异常处理人格占位
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
