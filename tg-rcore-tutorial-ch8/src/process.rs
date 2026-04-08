//! 进程与线程管理模块
//!
//! ## 与第七章的区别
//!
//! 第七章中 `Process` 既是资源容器又是执行单元。
//! 第八章将两者分离：
//! - **Process**：资源容器，管理地址空间、文件描述符、**同步原语列表**、信号
//! - **Thread**：执行单元，管理 TID 和上下文
//!
//! 同一进程的所有线程共享 `Process` 中的资源。
//!
//! ## 新增字段
//!
//! | 字段 | 说明 |
//! |------|------|
//! | `semaphore_list` | 信号量列表（进程内所有线程共享） |
//! | `mutex_list` | 互斥锁列表 |
//! | `condvar_list` | 条件变量列表 |
//!
//! 教程阅读建议：
//!
//! - 先看 `Process` 与 `Thread` 的字段分工：明确“资源归进程、执行归线程”；
//! - 再看 `fork/exec/from_elf`：理解跨线程模型后，进程复制与替换语义如何变化；
//! - 最后结合 `processor.rs` 看线程生命周期与进程资源回收的关系。

use crate::{
    build_flags, fs::Fd, map_portal, parse_flags, processor::ProcessorInner, Sv39, Sv39Manager,
    PROCESSOR,
};
use alloc::{alloc::alloc_zeroed, boxed::Box, sync::Arc, vec::Vec};
use core::alloc::Layout;
use spin::Mutex;
use tg_kernel_context::{foreign::ForeignContext, LocalContext};
use tg_kernel_vm::{
    page_table::{MmuMeta, VAddr, PPN, VPN},
    AddressSpace,
};
use tg_signal::Signal;
use tg_signal_impl::SignalImpl;
use tg_sync::{Condvar, Mutex as MutexTrait, Semaphore};
use tg_task_manage::{ProcId, ThreadId};
use xmas_elf::{
    header::{self, HeaderPt2, Machine},
    program, ElfFile,
};

/// 将 VirtIO 帧缓冲物理页映射到 `gpu::USER_FRAMEBUFFER_VA`，与 `get_fb_info` 返回的指针一致。
#[cfg(target_arch = "riscv64")]
fn map_user_framebuffer(address_space: &mut AddressSpace<Sv39, Sv39Manager>) {
    let pa = crate::gpu::get_framebuffer_ptr();
    let len = crate::gpu::get_framebuffer_len();
    if pa == 0 || len == 0 {
        println!("[DEBUG] map_user_framebuffer: GPU not ready (pa=0x{:x}, len={})", pa, len);
        return;
    }
    const PS: usize = 1 << Sv39::PAGE_BITS;
    let pa0 = pa & !(PS - 1);
    let span_bytes = pa + len - pa0;
    let np = span_bytes.div_ceil(PS);
    let va0 = crate::gpu::USER_FRAMEBUFFER_VA;
    println!("[DEBUG] map_user_framebuffer: pa=0x{:x} -> va=0x{:x}, {} pages, pa0=0x{:x}", pa, va0, np, pa0);
    assert!(va0 & (PS - 1) == 0, "USER_FRAMEBUFFER_VA must be page-aligned");
    let s = VAddr::<Sv39>::new(va0);
    let e = VAddr::<Sv39>::new(va0 + np * PS);
    address_space.map_extern(
        s.floor()..e.ceil(),
        PPN::new(pa0 >> Sv39::PAGE_BITS),
        build_flags("U_WRV"),
    );
    println!("[DEBUG] map_user_framebuffer: done");
}

/// 线程（执行单元）
///
/// 每个线程有独立的 TID 和上下文（寄存器状态、satp）。
/// 同一进程的多个线程共享地址空间。
pub struct Thread {
    /// 线程 ID（不可变）
    pub tid: ThreadId,
    /// 执行上下文（包含 LocalContext + satp）
    pub context: ForeignContext,
}

impl Thread {
    /// 创建新线程
    pub fn new(satp: usize, context: LocalContext) -> Self {
        Self {
            tid: ThreadId::new(),
            context: ForeignContext { context, satp },
        }
    }
}

/// 进程（资源容器）
///
/// 管理地址空间、文件描述符、同步原语、信号等共享资源。
/// 一个进程可以包含多个线程。
pub struct Process {
    /// 进程 ID
    pub pid: ProcId,
    /// 地址空间（所有线程共享）
    pub address_space: AddressSpace<Sv39, Sv39Manager>,
    /// 文件描述符表（所有线程共享）
    pub fd_table: Vec<Option<Mutex<Fd>>>,
    /// 信号处理器
    pub signal: Box<dyn Signal>,
    /// 信号量列表（**本章新增**，所有线程共享）
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// 互斥锁列表（**本章新增**，所有线程共享）
    pub mutex_list: Vec<Option<Arc<dyn MutexTrait>>>,
    /// 条件变量列表（**本章新增**，所有线程共享）
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// 是否启用死锁检测（进程级别）
    pub deadlock_detect: bool,
}

impl Process {
    /// exec：替换当前进程的地址空间和主线程上下文
    ///
    /// 注意：只支持单线程进程执行 exec
    pub fn exec(&mut self, elf: ElfFile) {
        println!("[DEBUG] exec: === START === pid={}", self.pid.get_usize());
        
        // 获取 exec 前的 satp
        let processor_before: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
        unsafe {
            let threads_before = (*processor_before).get_thread(self.pid).unwrap();
            let task_before = (*processor_before).get_task(threads_before[0]).unwrap();
            let satp_before = task_before.context.satp;
            println!("[DEBUG] exec: BEFORE - satp=0x{:x}", satp_before);
        }
        
        // exec 前验证 framebuffer 是否可访问
        #[cfg(target_arch = "riscv64")]
        {
            let fb_va = crate::gpu::USER_FRAMEBUFFER_VA;
            let can_read_before = self.address_space.translate::<u8>(
                VAddr::new(fb_va),
                build_flags("RV")
            ).is_some();
            println!("[DEBUG] exec: fb_va=0x{:x} readable before exec={}", fb_va, can_read_before);
        }
        
        let (proc, thread) = Process::from_elf(elf).unwrap();
        println!("[DEBUG] exec: from_elf created new address space, new satp=0x{:x}", thread.context.satp);
        
        // 替换地址空间
        println!("[DEBUG] exec: replacing address_space...");
        self.address_space = proc.address_space;
        println!("[DEBUG] exec: address_space replaced");
        
        // 更新线程上下文
        let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
        unsafe {
            let pthreads = (*processor).get_thread(self.pid).unwrap();
            let old_satp = (*processor).get_task(pthreads[0]).unwrap().context.satp;
            println!("[DEBUG] exec: old satp=0x{:x}, new satp=0x{:x}", old_satp, thread.context.satp);
            (*processor).get_task(pthreads[0]).unwrap().context = thread.context;
            let new_satp = (*processor).get_task(pthreads[0]).unwrap().context.satp;
            println!("[DEBUG] exec: AFTER UPDATE - satp=0x{:x}", new_satp);
        }
        
        // exec 后验证新栈是否可访问
        #[cfg(target_arch = "riscv64")]
        {
            let stack_va = 0x3fffffe000usize; // doom 的栈顶
            let can_read_stack = self.address_space.translate::<u8>(VAddr::new(stack_va), build_flags("RV")).is_some();
            let can_write_stack = self.address_space.translate::<u8>(VAddr::new(stack_va), build_flags("W_V")).is_some();
            println!("[DEBUG] exec: stack_va=0x{:x} readable={}, writable={}", stack_va, can_read_stack, can_write_stack);
        }
        
        // exec 后再次验证 framebuffer
        #[cfg(target_arch = "riscv64")]
        {
            let fb_va = crate::gpu::USER_FRAMEBUFFER_VA;
            let can_read_after = self.address_space.translate::<u8>(
                VAddr::new(fb_va),
                build_flags("RV")
            ).is_some();
            println!("[DEBUG] exec: fb_va=0x{:x} readable after exec={}", fb_va, can_read_after);
        }
        println!("[DEBUG] exec: === END ===");
    }

    /// fork：创建子进程（复制地址空间和主线程上下文）
    ///
    /// 子进程继承父进程的地址空间（深拷贝）、文件描述符和信号配置。
    /// 同步原语列表不继承（子进程创建空的列表）。
    pub fn fork(&mut self) -> Option<(Self, Thread)> {
        let pid = ProcId::new();
        println!("[DEBUG] fork: parent pid={}, new child pid={}", self.pid.get_usize(), pid.get_usize());
        
        // 深拷贝地址空间
        let parent_addr_space = &self.address_space;
        let mut address_space: AddressSpace<Sv39, Sv39Manager> = AddressSpace::new();
        
        // cloneself 前的父进程页表信息
        let parent_satp = (8 << 60) | parent_addr_space.root_ppn().val();
        println!("[DEBUG] fork: parent address_space root_ppn=0x{:x}, satp=0x{:x}", parent_addr_space.root_ppn().val(), parent_satp);
        
        println!("[DEBUG] fork: starting cloneself...");
        parent_addr_space.cloneself(&mut address_space);
        println!("[DEBUG] fork: cloneself done");
        
        let child_satp = (8 << 60) | address_space.root_ppn().val();
        println!("[DEBUG] fork: child address_space root_ppn=0x{:x}, satp=0x{:x}", address_space.root_ppn().val(), child_satp);
        
        map_portal(&address_space);
        // 复制主线程上下文
        let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
        let pthreads = unsafe { (*processor).get_thread(self.pid).unwrap() };
        let context = unsafe {
            (*processor).get_task(pthreads[0]).unwrap().context.context.clone()
        };
        let satp = (8 << 60) | address_space.root_ppn().val();
        println!("[DEBUG] fork: creating Thread with satp=0x{:x}", satp);
        let thread = Thread::new(satp, context);
        
        // 复制文件描述符表
        let new_fd_table: Vec<Option<Mutex<Fd>>> = self.fd_table
            .iter()
            .map(|fd| fd.as_ref().map(|f| Mutex::new(f.lock().clone())))
            .collect();
        println!("[DEBUG] fork: done, returning child process and thread");
        Some((
            Self {
                pid,
                address_space,
                fd_table: new_fd_table,
                signal: self.signal.from_fork(),
                // 子进程的同步原语列表初始为空
                semaphore_list: Vec::new(),
                mutex_list: Vec::new(),
                condvar_list: Vec::new(),
                deadlock_detect: false,
            },
            thread,
        ))
    }

    /// 从 ELF 文件创建进程和主线程
    ///
    /// 解析 ELF 段，建立地址空间，分配用户栈，创建初始上下文。
    pub fn from_elf(elf: ElfFile) -> Option<(Self, Thread)> {
        println!("[DEBUG] from_elf: === START ===");
        
        let entry = match elf.header.pt2 {
            HeaderPt2::Header64(pt2)
                if pt2.type_.as_type() == header::Type::Executable
                    && pt2.machine.as_machine() == Machine::RISC_V =>
            { pt2.entry_point as usize }
            _ => None?,
        };
        println!("[DEBUG] from_elf: entry point = 0x{:x}", entry);

        const PAGE_SIZE: usize = 1 << Sv39::PAGE_BITS;
        const PAGE_MASK: usize = PAGE_SIZE - 1;

        let mut address_space = AddressSpace::new();
        let mut segment_count = 0usize;
        for program in elf.program_iter() {
            if !matches!(program.get_type(), Ok(program::Type::Load)) { continue; }
            segment_count += 1;
            let off_file = program.offset() as usize;
            let len_file = program.file_size() as usize;
            let off_mem = program.virtual_addr() as usize;
            let end_mem = off_mem + program.mem_size() as usize;
            println!("[DEBUG] from_elf: segment[{}] - va=0x{:x}..0x{:x}, file_off=0x{:x}, file_sz=0x{:x}, mem_sz=0x{:x}", 
                segment_count, off_mem, end_mem, off_file, len_file, program.mem_size());
            assert_eq!(off_file & PAGE_MASK, off_mem & PAGE_MASK);
            let mut flags: [u8; 5] = *b"U___V";
            if program.flags().is_execute() { flags[1] = b'X'; }
            if program.flags().is_write() { flags[2] = b'W'; }
            if program.flags().is_read() { flags[3] = b'R'; }
            address_space.map(
                VAddr::new(off_mem).floor()..VAddr::new(end_mem).ceil(),
                &elf.input[off_file..][..len_file],
                off_mem & PAGE_MASK,
                parse_flags(unsafe { core::str::from_utf8_unchecked(&flags) }).unwrap(),
            );
        }
        println!("[DEBUG] from_elf: mapped {} load segments", segment_count);
        
        // 分配 2 页用户栈
        let stack_size = 2 << Sv39::PAGE_BITS;
        let stack = unsafe {
            alloc_zeroed(Layout::from_size_align_unchecked(
                stack_size, 1 << Sv39::PAGE_BITS,
            ))
        };
        let stack_pa = stack as usize;
        let stack_ppn = stack_pa >> Sv39::PAGE_BITS;
        let vpn_start = (1 << 26) - 2;
        let vpn_end = 1 << 26;
        let va_start = vpn_start * PAGE_SIZE;
        let va_end = vpn_end * PAGE_SIZE;
        
        println!("[DEBUG] from_elf: allocating STACK:");
        println!("[DEBUG] from_elf:   stack_va = 0x{:x}..0x{:x} (VPN 0x{:x}..0x{:x})", va_start, va_end, vpn_start, vpn_end);
        println!("[DEBUG] from_elf:   stack_pa = 0x{:x}, stack_ppn = 0x{:x}", stack_pa, stack_ppn);
        println!("[DEBUG] from_elf:   stack_size = 0x{:x} bytes ({} pages)", stack_size, stack_size / PAGE_SIZE);
        
        if stack_pa == 0 {
            println!("[ERROR] from_elf: stack allocation FAILED! stack_pa = 0");
        }
        
        address_space.map_extern(
            VPN::new(vpn_start)..VPN::new(vpn_end),
            PPN::new(stack_ppn),
            build_flags("U_WRV"),
        );
        println!("[DEBUG] from_elf: stack mapped via map_extern");
        
        // 验证栈映射
        let stack_va_check = va_end - 8; // 栈顶附近
        let stack_translated = address_space.translate::<u8>(VAddr::new(stack_va_check), build_flags("W_V"));
        println!("[DEBUG] from_elf: verifying stack at va=0x{:x}: {:?}", stack_va_check, stack_translated.is_some());
        
        map_portal(&address_space);
        #[cfg(target_arch = "riscv64")]
        map_user_framebuffer(&mut address_space);
        // 验证 framebuffer 映射是否生效
        #[cfg(target_arch = "riscv64")]
        {
            let fb_va = crate::gpu::USER_FRAMEBUFFER_VA;
            let fb_ptr = crate::gpu::user_framebuffer_ptr();
            println!("[DEBUG] from_elf: fb_va=0x{:x}, user_fb_ptr=0x{:x}", fb_va, fb_ptr);
            let can_read = address_space.translate::<u8>(VAddr::new(fb_va), build_flags("RV")).is_some();
            let can_write = address_space.translate::<u8>(VAddr::new(fb_va), build_flags("W_V")).is_some();
            println!("[DEBUG] from_elf: fb_va readable={}, writable={}", can_read, can_write);
        }
        
        let satp = (8 << 60) | address_space.root_ppn().val();
        println!("[DEBUG] from_elf: creating LocalContext:");
        println!("[DEBUG] from_elf:   entry = 0x{:x}", entry);
        let sp_val: usize = 1 << 38;
        println!("[DEBUG] from_elf:   sp = 0x{:x} (1 << 38)", sp_val);
        println!("[DEBUG] from_elf:   satp = 0x{:x}", satp);
        println!("[DEBUG] from_elf:   root_ppn = 0x{:x}", address_space.root_ppn().val());
        
        let mut context = LocalContext::user(entry);
        *context.sp_mut() = 1 << 38;
        let thread = Thread::new(satp, context);

        println!("[DEBUG] from_elf: === END === (returning new process with Thread)");
        Some((
            Self {
                pid: ProcId::new(),
                address_space,
                fd_table: vec![
                    // stdin
                    Some(Mutex::new(Fd::Empty { read: true, write: false })),
                    // stdout
                    Some(Mutex::new(Fd::Empty { read: false, write: true })),
                    // stderr
                    Some(Mutex::new(Fd::Empty { read: false, write: true })),
                ],
                signal: Box::new(SignalImpl::new()),
                semaphore_list: Vec::new(),
                mutex_list: Vec::new(),
                condvar_list: Vec::new(),
                deadlock_detect: false,
            },
            thread,
        ))
    }
}
