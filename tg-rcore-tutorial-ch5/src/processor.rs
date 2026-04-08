//! 处理器管理模块
//!
//! 定义 `PROCESSOR` 全局变量和 `ProcManager` 进程管理器。
//!
//! ## 设计思路
//!
//! 进程管理分为两部分：
//! - `PROCESSOR`：封装 `PManager`，提供全局访问接口，管理当前运行的进程
//! - `ProcManager`：实现 `Manage` 和 `Schedule` trait，支持可插拔的调度算法

use crate::process::Process;
use crate::scheduler::{Scheduler, SchedulerType, create_scheduler};
use alloc::collections::BTreeMap;
use alloc::boxed::Box;
use core::cell::UnsafeCell;
use tg_task_manage::{Manage, PManager, ProcId, Schedule};

/// 处理器全局管理器
///
/// 封装 `PManager<Process, ProcManager>`，通过 `UnsafeCell` 提供内部可变性。
/// 在单核环境下是安全的，因为不会出现并发访问。
pub struct Processor {
    inner: UnsafeCell<PManager<Process, ProcManager>>,
}

unsafe impl Sync for Processor {}

impl Processor {
    /// 创建新的处理器管理器（编译期常量初始化）
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(PManager::new()),
        }
    }

    /// 获取内部 PManager 的可变引用
    #[inline]
    pub fn get_mut(&self) -> &mut PManager<Process, ProcManager> {
        unsafe { &mut (*self.inner.get()) }
    }
}

/// 全局处理器管理器实例
pub static PROCESSOR: Processor = Processor::new();

/// 进程管理器
///
/// 负责管理所有进程实体和可插拔的调度器：
/// - `tasks`：以 ProcId 为键的进程映射表，存储所有进程实体
/// - `scheduler`：可插拔的调度算法实现
pub struct ProcManager {
    /// 所有进程实体的映射表
    tasks: BTreeMap<ProcId, Process>,
    /// 可插拔的调度器
    scheduler: Box<dyn Scheduler>,
    /// 当前运行的进程ID
    current_process: Option<ProcId>,
    /// 系统时钟周期
    current_time: usize,
}

impl ProcManager {
    /// 创建新的进程管理器
    pub fn new() -> Self {
        let default_scheduler = Self::default_scheduler();
        
        Self {
            tasks: BTreeMap::new(),
            scheduler: default_scheduler,
            current_process: None,
            current_time: 0,
        }
    }

    /// 运行时默认调度器（始终为RR，允许运行时切换）
    fn default_scheduler() -> Box<dyn Scheduler> {
        // 移除编译时feature门控，改为运行时选择
        // 默认使用RR调度器（时间片10）
        create_scheduler(SchedulerType::RR(10))
    }

    /// 创建指定调度器的进程管理器
    #[allow(dead_code)]
    pub fn with_scheduler(scheduler_type: SchedulerType) -> Self {
        Self {
            tasks: BTreeMap::new(),
            scheduler: create_scheduler(scheduler_type),
            current_process: None,
            current_time: 0,
        }
    }

    /// 切换调度器
    #[allow(dead_code)]
    pub fn switch_scheduler(&mut self, scheduler_type: SchedulerType) {
        // 将现有队列中的所有进程重新加入新调度器
        let current_pids: alloc::vec::Vec<_> = 
            self.tasks.keys().copied().collect();
        self.scheduler = create_scheduler(scheduler_type);
        for pid in current_pids {
            self.scheduler.enqueue(pid);
        }
    }

    /// 获取调度器的统计信息
    #[allow(dead_code)]
    pub fn get_scheduler_stats(&self) -> &crate::scheduler::SchedulerStats {
        self.scheduler.get_stats()
    }

    /// 更新系统时钟
    #[allow(dead_code)]
    pub fn tick(&mut self) {
        self.current_time += 1;
        self.scheduler.on_tick(self.current_process, self.current_time);
    }
}

/// 实现 Manage trait：进程实体的增删查
impl Manage<Process, ProcId> for ProcManager {
    /// 插入新进程到进程表
    #[inline]
    fn insert(&mut self, id: ProcId, task: Process) {
        self.tasks.insert(id, task);
    }

    /// 根据 PID 获取进程的可变引用
    #[inline]
    fn get_mut(&mut self, id: ProcId) -> Option<&mut Process> {
        self.tasks.get_mut(&id)
    }

    /// 从进程表中删除进程（回收资源）
    #[inline]
    fn delete(&mut self, id: ProcId) {
        self.tasks.remove(&id);
    }
}

/// 实现 Schedule trait：进程调度（支持可插拔的调度算法）
impl Schedule<ProcId> for ProcManager {
    /// 将进程加入就绪队列
    fn add(&mut self, id: ProcId) {
        self.scheduler.enqueue(id);
    }

    /// 从就绪队列中选出下一个要执行的进程
    fn fetch(&mut self) -> Option<ProcId> {
        let next = self.scheduler.pick_next();
        self.current_process = next;
        next
    }
}
