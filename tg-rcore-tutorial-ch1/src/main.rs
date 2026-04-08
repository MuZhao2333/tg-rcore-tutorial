//! # 第一章：应用程序与基本执行环境
//!
//! 本章实现了一个最简单的 RISC-V S 态裸机程序，展示操作系统的最小执行环境。
//!
//! ## 关键概念
//!
//! - `#![no_std]`：不使用 Rust 标准库，改用不依赖操作系统的核心库 `core`
//! - `#![no_main]`：不使用标准的 `main` 入口，自定义裸函数 `_start` 作为入口
//! - 裸函数（naked function）：不生成函数序言/尾声，可在无栈环境下执行
//! - SBI（Supervisor Binary Interface）：S 态软件向 M 态固件请求服务的标准接口
//!
//! ## 扩展：时钟中断
//!
//! 在第一章的基础上，我们还展示了**时钟中断**的处理机制：
//!
//! - **时钟中断**：由硬件计时器触发，是实现抢占式调度的基础
//! - **CSR（Control and Status Register）**：RISC-V 控制状态寄存器，用于配置中断和查询中断原因
//! - **定时器设置**：通过 SBI 调用 `set_timer` 设置下一次中断时间
//!
//! 教程阅读建议：
//!
//! - 先看 `_start`：理解无运行时情况下的最小启动流程；
//! - 再看 `rust_main`：理解最小 I/O 路径（SBI 输出 + 关机）；
//! - 最后看 `panic_handler`：理解 no_std 程序的异常收口方式。

// 不使用标准库，因为裸机环境没有操作系统提供系统调用支持
#![no_std]
// 不使用标准入口，因为裸机环境没有 C runtime 进行初始化
#![no_main]
// RISC-V64 架构下启用严格警告和文档检查
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
// 非 RISC-V64 架构允许死代码（用于 cargo publish --dry-run 在主机上通过编译）
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

// 引入 SBI 调用库，提供 console_putchar（输出字符）和 shutdown（关机）功能
// 启用 nobios 特性后，tg_sbi 内建了 M-mode 启动代码，无需外部 SBI 固件
use tg_sbi::{console_putchar, shutdown};

// 引入 RISC-V 寄存器访问库，提供 CSR 操作（用于时钟中断）
use riscv::register::{sie, sstatus, stvec, time};

// ========== Trap 入口点（汇编实现）==========
// 
// stvec 指向 trap_entry。当 S-mode 发生 trap 时，CPU 跳转到这里。
// trap_entry 是一个汇编标签，它：
// 1. 调用 Rust 侧的 trap_handler 函数
// 2. 通过 sret 指令返回到 trap 点之后的地址
//
// 这种实现方式避免了 naked_asm 的复杂性。
core::arch::global_asm!(
    r#"
    .section .text.trap
    .globl trap_entry
    .align 4
trap_entry:
    // 保存 ra 和 t0 到栈（其他寄存器暂不保存，因为 trap_handler 自己保存）
    addi sp, sp, -16
    sd   ra, 0(sp)
    sd   t0, 8(sp)
    
    // 调用 Rust 风格的 trap handler
    call {trap_handler}
    
    // 恢复寄存器
    ld   ra, 0(sp)
    ld   t0, 8(sp)
    addi sp, sp, 16
    
    // sret 指令返回到触发 trap 的位置
    sret
    "#,
    trap_handler = sym _trap,
);

// ========== 常量定义 ==========

/// 时钟中断间隔：每 10,000,000 个时钟周期触发一次
///
/// QEMU virt 平台的时钟频率为 10 MHz（10,000,000 Hz），
/// 因此这个设置大约每秒触发一次时钟中断。
const TIMER_INTERVAL: u64 = 10_000_000;

/// S 态程序入口点。
///
/// 这是一个裸函数（naked function），放置在 `.text.entry` 段，
/// 链接脚本将其安排在地址 `0x80200000`。
///
/// 裸函数不生成函数序言和尾声，因此可以在没有栈的情况下执行。
/// 它完成两件事：
/// 1. 设置栈指针 `sp`，指向栈顶（栈从高地址向低地址增长）
/// 2. 跳转到 Rust 主函数 `rust_main`
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    // 栈大小：4 KiB
    const STACK_SIZE: usize = 4096;

    // 在 .bss.uninit 段中分配栈空间
    #[unsafe(link_section = ".bss.uninit")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}", // 将 sp 设置为栈顶地址
        "j  {main}",                      // 跳转到 rust_main
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        main       =   sym rust_main,
    )
}

/// S 态主函数：初始化时钟中断，然后进入主循环。
///
/// 与简单的 "Hello, world!" 不同，这里进入一个无限循环，
/// 等待并处理时钟中断。每次中断时输出一个字符，形成"tick-tock"的定时输出效果。
extern "C" fn rust_main() -> ! {
    // ========== 调试：输出初始化信息 ==========
    for c in b"Init...\n" {
        console_putchar(*c);
    }

    // 第一步：使能 S-mode 全局中断（SIE 位）
    // 这允许 CPU 响应各种 S-mode 中断，包括时钟中断
    // SAFETY: 设置 sstatus CSR 是 S-mode 下允许的操作
    unsafe { sstatus::set_sie() };

    // 调试：检查 sstatus（使用 csrr 指令直接读取）
    for c in b"sstatus: " { console_putchar(*c); }
    print_hex(read_sstatus());

    // 第二步：使能 S-mode 时钟中断
    // 这告诉硬件：允许时钟中断传递到 S-mode
    // 注意：中断委托在 M-mode 的 m_entry.asm 中配置
    // SAFETY: 设置 sie CSR 是 S-mode 下允许的操作，用于使能时钟中断
    unsafe { sie::set_stimer() };

    // 调试：检查 sie（使用 csrr 指令直接读取）
    for c in b"sie: " { console_putchar(*c); }
    print_hex(read_sie());

    // 第三步：设置第一次时钟中断（使用较小的间隔以便测试）
    use tg_sbi::set_timer;
    let current_time = time::read64() as usize;
    for c in b"time: " { console_putchar(*c); }
    print_hex(current_time);

    // 设置一个较短的时间间隔（1秒 = 10_000_000 个周期）
    let mut next_interrupt = current_time + TIMER_INTERVAL as usize;
    for c in b"mtimecmp: " { console_putchar(*c); }
    print_hex(next_interrupt);
    set_timer(next_interrupt as u64);

    // 第四步：设置 S-mode trap 向量
    // stvec 的低位用于指定 trap 处理方式（0=Direct, 1=Vectored）
    // 这里设置为 Direct 模式，所有 trap 都跳转到同一个处理程序
    // trap_entry 由 global_asm! 定义，是真正的汇编入口点
    // SAFETY: 设置 stvec 是 S-mode 下允许的操作
    unsafe {
        unsafe extern "C" {
            #[link_name = "trap_entry"]
            fn trap_entry();
        }
        stvec::write(trap_entry as *const () as usize, stvec::TrapMode::Direct);
    }

    for c in b"stvec set, entering interrupt-driven mode...\n" { console_putchar(*c); }

    // 第五步：轮询模式测试（用于诊断时间增长问题）
    // 让我们先用轮询而不是 wfi 来看时间是否真的在增长
    for c in b"Polling mode (testing)...\n" { console_putchar(*c); }
    
    let mut count = 0;
    let mut loop_count = 0;
    
    loop {
        let now = time::read64() as usize;
        
        // 每 100_000_000 次循环打印一次时间（以降低输出频率）
        loop_count += 1;
        if loop_count >= 100_000_000 {
            loop_count = 0;
            for c in b"time: " { console_putchar(*c); }
            print_hex(now);
        }
        
        // 检查是否达到下一次中断时间
        if now >= next_interrupt {
            for c in b"\nTimer! " { console_putchar(*c); }
            print_hex(now);
            
            // 设置下一次中断
            next_interrupt = now + TIMER_INTERVAL as usize;
            set_timer(next_interrupt as u64);
            count += 1;
            
            if count >= 5 {
                for c in b"\nDone (5 ticks)\n" { console_putchar(*c); }
                shutdown(false);
            }
        }
    }
}

/// 打印 16 进制数字（简化版，不使用格式化宏）
fn print_hex(val: usize) {
    // 输出 "0x" 前缀
    console_putchar(b'0');
    console_putchar(b'x');

    // 计算有多少位（最多 16 位）
    if val == 0 {
        console_putchar(b'0');
        console_putchar(b'\n');
        return;
    }

    // 找出最高位的位置
    let mut digits = [0u8; 16];
    let mut count = 0;
    let mut v = val;
    while v > 0 {
        digits[count] = (v & 0xF) as u8;
        v >>= 4;
        count += 1;
    }

    // 反向输出
    let hex_chars = b"0123456789abcdef";
    while count > 0 {
        count -= 1;
        console_putchar(hex_chars[digits[count] as usize]);
    }
    console_putchar(b'\n');
}

/// 使用内联汇编读取 sstatus CSR (0x100)
fn read_sstatus() -> usize {
    let val: usize;
    unsafe { core::arch::asm!("csrr {0}, sstatus", out(reg) val); }
    val
}

/// 使用内联汇编读取 sie CSR (0x104)
fn read_sie() -> usize {
    let val: usize;
    unsafe { core::arch::asm!("csrr {0}, sie", out(reg) val); }
    val
}

/// S-mode trap 处理程序
/// 
/// stvec 指向这个函数。所有 S-mode trap（包括时钟中断）都会跳转到这里。
/// 
/// 该函数：
/// 1. 在汇编末尾保存关键寄存器
/// 2. 调用 Rust 处理函数 trap_handler  
/// 3. 通过 sret 返回（继续执行 trap 点之后的代码）
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text")]
extern "C" fn _trap() {
    // 读取 trap 原因
    let scause: usize;
    unsafe {
        core::arch::asm!("csrr {}, scause", out(reg) scause);
    }
    
    // 判断是否为时钟中断（高位为1表示中断，位5的中断是时钟中断）
    let is_interrupt = (scause >> 63) != 0;
    let cause_code = scause & 0x3f;
    
    if is_interrupt && cause_code == 5 {
        // 时钟中断处理
        static mut TRAP_COUNT: usize = 0;
        
        let count = unsafe {
            TRAP_COUNT += 1;
            TRAP_COUNT
        };
        
        // 输出定时器信息
        for c in b"\nTimer! " {
            console_putchar(*c);
        }
        print_hex(time::read64() as usize);
        
        // 设置下一次定时器中断
        use tg_sbi::set_timer;
        let next = time::read64() as u64 + TIMER_INTERVAL;
        set_timer(next);
        
        // 如果已经收到 5 次中断，则关机
        if count >= 5 {
            for c in b"\nDone (5 ticks)\n" {
                console_putchar(*c);
            }
            shutdown(false);
        }
    } else {
        // 其他 trap：输出错误并关机
        for c in b"\nUnexpected trap! scause=" {
            console_putchar(*c);
        }
        print_hex(scause);
        shutdown(true);
    }
}


/// panic 处理函数。
///
/// `#![no_std]` 环境下必须自行实现。发生 panic 时以异常状态关机。
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    shutdown(true) // true 表示异常关机
}

/// 非 RISC-V64 架构的占位模块。
///
/// 提供 `main` 等符号，使得在主机平台（如 x86_64）上也能通过编译，
/// 满足 `cargo publish --dry-run` 和 `cargo test` 的需求。
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
