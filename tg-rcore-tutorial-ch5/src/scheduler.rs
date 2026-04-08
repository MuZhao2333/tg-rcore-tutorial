//! 可插拔调度器框架
//!
//! 定义统一的调度器接口和各种调度算法实现，以及性能数据采集框架。
//!
//! ## 支持的调度算法
//! - FCFS: 先来先服务
//! - SJF: 最短作业优先（估计）
//! - RR: 时间片轮转
//! - MLFQ: 多级反馈队列
//! - CFS: 完全公平调度（简化版）

#![allow(dead_code)]

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::fmt::Debug;
use tg_task_manage::ProcId;

/// 调度器统计数据
#[derive(Clone, Debug, Default)]
pub struct SchedulerStats {
    /// 上下文切换总次数
    pub context_switches: usize,
    /// 当前就绪队列长度
    pub queue_length: usize,
    /// 各进程的等待时间
    pub wait_times: BTreeMap<ProcId, usize>,
    /// 各进程的周转时间
    pub turnaround_times: BTreeMap<ProcId, usize>,
    /// 各进程的运行次数
    pub run_counts: BTreeMap<ProcId, usize>,
    /// 饥饿计数：进程等待超过阈值的次数
    pub starvation_counts: BTreeMap<ProcId, usize>,
    /// 时间戳（用于计算等待时间）
    pub timestamps: BTreeMap<ProcId, usize>,
}

/// 统一的调度器接口
pub trait Scheduler {
    /// 将进程加入就绪队列
    fn enqueue(&mut self, id: ProcId);

    /// 从就绪队列中选取下一个要执行的进程
    fn pick_next(&mut self) -> Option<ProcId>;

    /// 时钟中断处理：可用于时间片检查或其他周期性更新
    fn on_tick(&mut self, current: Option<ProcId>, current_time: usize);

    /// 进程阻塞时的回调
    fn on_block(&mut self, id: ProcId, current_time: usize);

    /// 进程唤醒时的回调
    fn on_wakeup(&mut self, id: ProcId, current_time: usize);

    /// 获取统计信息
    fn get_stats(&self) -> &SchedulerStats;

    /// 获取可变统计信息
    fn get_stats_mut(&mut self) -> &mut SchedulerStats;

    /// 检查队列是否为空
    fn is_empty(&self) -> bool;

    /// 获取队列长度
    fn queue_len(&self) -> usize;
}

/// FCFS（先来先服务）调度器
pub struct FCFSScheduler {
    ready_queue: VecDeque<ProcId>,
    stats: SchedulerStats,
}

impl FCFSScheduler {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            stats: SchedulerStats::default(),
        }
    }
}

impl Scheduler for FCFSScheduler {
    fn enqueue(&mut self, id: ProcId) {
        if !self.ready_queue.contains(&id) {
            self.ready_queue.push_back(id);
        }
        if let Some(t) = self.stats.timestamps.get(&id) {
            if *t == 0 {
                self.stats.timestamps.insert(id, 0); // 记录入队时间
            }
        } else {
            self.stats.timestamps.insert(id, 0);
        }
    }

    fn pick_next(&mut self) -> Option<ProcId> {
        let id = self.ready_queue.pop_front();
        if id.is_some() {
            self.stats.context_switches += 1;
        }
        id
    }

    fn on_tick(&mut self, _current: Option<ProcId>, _current_time: usize) {
        // FCFS不需要时间片管理
    }

    fn on_block(&mut self, _id: ProcId, _current_time: usize) {
        // 不需要额外处理
    }

    fn on_wakeup(&mut self, id: ProcId, _current_time: usize) {
        self.enqueue(id);
    }

    fn get_stats(&self) -> &SchedulerStats {
        &self.stats
    }

    fn get_stats_mut(&mut self) -> &mut SchedulerStats {
        &mut self.stats
    }

    fn is_empty(&self) -> bool {
        self.ready_queue.is_empty()
    }

    fn queue_len(&self) -> usize {
        self.ready_queue.len()
    }
}

/// SJF（最短作业优先）调度器 - 使用预测运行时间
pub struct SJFScheduler {
    ready_queue: VecDeque<ProcId>,
    stats: SchedulerStats,
    predicted_times: BTreeMap<ProcId, usize>,
}

impl SJFScheduler {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            stats: SchedulerStats::default(),
            predicted_times: BTreeMap::new(),
        }
    }

    /// 获取进程的预测运行时间
    fn get_predicted_time(&self, id: ProcId) -> usize {
        self.predicted_times
            .get(&id)
            .copied()
            .unwrap_or(100) // 默认预测100个时间单位
    }

    /// 更新预测运行时间（简单指数移动平均）
    fn update_predicted_time(&mut self, id: ProcId, actual_time: usize) {
        let old_pred = self.get_predicted_time(id);
        let new_pred = (old_pred * 7 + actual_time) / 8; // 加权平均
        self.predicted_times.insert(id, new_pred);
    }
}

impl Scheduler for SJFScheduler {
    fn enqueue(&mut self, id: ProcId) {
        if !self.ready_queue.contains(&id) {
            // 插入到合适的位置（按预测时间排序）
            let predicted_time = self.get_predicted_time(id);
            let mut inserted = false;
            for (i, &pid) in self.ready_queue.iter().enumerate() {
                if predicted_time < self.get_predicted_time(pid) {
                    self.ready_queue.insert(i, id);
                    inserted = true;
                    break;
                }
            }
            if !inserted {
                self.ready_queue.push_back(id);
            }
        }
    }

    fn pick_next(&mut self) -> Option<ProcId> {
        let id = self.ready_queue.pop_front();
        if id.is_some() {
            self.stats.context_switches += 1;
        }
        id
    }

    fn on_tick(&mut self, _current: Option<ProcId>, _current_time: usize) {
        // SJF不强制时间片，但可以记录运行时间
    }

    fn on_block(&mut self, _id: ProcId, _current_time: usize) {}

    fn on_wakeup(&mut self, id: ProcId, _current_time: usize) {
        self.enqueue(id);
    }

    fn get_stats(&self) -> &SchedulerStats {
        &self.stats
    }

    fn get_stats_mut(&mut self) -> &mut SchedulerStats {
        &mut self.stats
    }

    fn is_empty(&self) -> bool {
        self.ready_queue.is_empty()
    }

    fn queue_len(&self) -> usize {
        self.ready_queue.len()
    }
}

/// RR（时间片轮转）调度器
pub struct RRScheduler {
    ready_queue: VecDeque<ProcId>,
    stats: SchedulerStats,
    time_slice: usize,
    current_slice_used: usize,
}

impl RRScheduler {
    pub fn new(time_slice: usize) -> Self {
        Self {
            ready_queue: VecDeque::new(),
            stats: SchedulerStats::default(),
            time_slice,
            current_slice_used: 0,
        }
    }
}

impl Scheduler for RRScheduler {
    fn enqueue(&mut self, id: ProcId) {
        if !self.ready_queue.contains(&id) {
            self.ready_queue.push_back(id);
        }
    }

    fn pick_next(&mut self) -> Option<ProcId> {
        let id = self.ready_queue.pop_front();
        if id.is_some() {
            self.stats.context_switches += 1;
            self.current_slice_used = 0;
        }
        id
    }

    fn on_tick(&mut self, current: Option<ProcId>, _current_time: usize) {
        self.current_slice_used += 1;
        // 如果当前进程用完了时间片，把它移到队列末尾
        if self.current_slice_used >= self.time_slice && current.is_some() {
            if let Some(current_pid) = current {
                self.ready_queue.push_back(current_pid);
                self.current_slice_used = 0;
            }
        }
    }

    fn on_block(&mut self, _id: ProcId, _current_time: usize) {
        self.current_slice_used = 0;
    }

    fn on_wakeup(&mut self, id: ProcId, _current_time: usize) {
        self.enqueue(id);
    }

    fn get_stats(&self) -> &SchedulerStats {
        &self.stats
    }

    fn get_stats_mut(&mut self) -> &mut SchedulerStats {
        &mut self.stats
    }

    fn is_empty(&self) -> bool {
        self.ready_queue.is_empty()
    }

    fn queue_len(&self) -> usize {
        self.ready_queue.len()
    }
}

/// MLFQ（多级反馈队列）调度器
/// 具有多个优先级队列，进程可以在队列间移动
pub struct MLFQScheduler {
    queues: Vec<VecDeque<ProcId>>,
    stats: SchedulerStats,
    time_slices: Vec<usize>,
    current_slice_used: usize,
    priority_levels: usize,
}

impl MLFQScheduler {
    pub fn new(priority_levels: usize) -> Self {
        let mut queues = Vec::new();
        for _ in 0..priority_levels {
            queues.push(VecDeque::new());
        }
        // 时间片随优先级递增（高优先级为低等级号）
        let time_slices: Vec<usize> = (0..priority_levels)
            .map(|i| 2 << (i as u32))
            .collect();

        Self {
            queues,
            stats: SchedulerStats::default(),
            time_slices,
            current_slice_used: 0,
            priority_levels,
        }
    }

    /// 获取进程的优先级（队列索引）
    fn get_priority(&self, _id: ProcId) -> usize {
        // 简化：所有进程始终分配到最低优先级队列
        // 也可以通过外部配置来动态分配优先级
        0
    }

    /// 提升进程优先级
    fn boost_priority(&self, priority: usize) -> usize {
        if priority > 0 {
            priority - 1
        } else {
            0
        }
    }

    /// 降低进程优先级
    fn lower_priority(&self, priority: usize) -> usize {
        if priority < self.priority_levels - 1 {
            priority + 1
        } else {
            self.priority_levels - 1
        }
    }
}

impl Scheduler for MLFQScheduler {
    fn enqueue(&mut self, id: ProcId) {
        let priority = self.get_priority(id);
        if !self.queues[priority].contains(&id) {
            self.queues[priority].push_back(id);
        }
    }

    fn pick_next(&mut self) -> Option<ProcId> {
        // 从最高优先级队列开始查找
        for i in 0..self.priority_levels {
            if let Some(id) = self.queues[i].pop_front() {
                self.stats.context_switches += 1;
                self.current_slice_used = 0;
                return Some(id);
            }
        }
        None
    }

    fn on_tick(&mut self, current: Option<ProcId>, _current_time: usize) {
        if let Some(current_pid) = current {
            self.current_slice_used += 1;
            let priority = self.get_priority(current_pid);
            let time_slice = self.time_slices[priority];

            // 如果用完时间片，降级并重新入队
            if self.current_slice_used >= time_slice {
                let new_priority = self.lower_priority(priority);
                self.queues[new_priority].push_back(current_pid);
                self.current_slice_used = 0;
            }
        }
    }

    fn on_block(&mut self, _id: ProcId, _current_time: usize) {
        self.current_slice_used = 0;
    }

    fn on_wakeup(&mut self, id: ProcId, _current_time: usize) {
        // 唤醒时提升优先级
        let priority = self.get_priority(id);
        let new_priority = self.boost_priority(priority);
        self.queues[new_priority].push_back(id);
    }

    fn get_stats(&self) -> &SchedulerStats {
        &self.stats
    }

    fn get_stats_mut(&mut self) -> &mut SchedulerStats {
        &mut self.stats
    }

    fn is_empty(&self) -> bool {
        self.queues.iter().all(|q| q.is_empty())
    }

    fn queue_len(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }
}

/// CFS（完全公平调度）- 简化版实现
/// 基于"虚拟运行时间"来追踪每个进程应该获得的CPU时间
pub struct CFSScheduler {
    ready_queue: Vec<ProcId>,
    stats: SchedulerStats,
    virtual_times: BTreeMap<ProcId, usize>,
}

impl CFSScheduler {
    pub fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
            stats: SchedulerStats::default(),
            virtual_times: BTreeMap::new(),
        }
    }

    /// 获取进程的虚拟运行时间
    fn get_vruntime(&self, id: ProcId) -> usize {
        self.virtual_times.get(&id).copied().unwrap_or(0)
    }

    /// 更新虚拟运行时间
    fn update_vruntime(&mut self, id: ProcId, delta: usize) {
        let current = self.get_vruntime(id);
        self.virtual_times.insert(id, current + delta);
    }
}

impl Scheduler for CFSScheduler {
    fn enqueue(&mut self, id: ProcId) {
        if !self.ready_queue.contains(&id) {
            self.ready_queue.push(id);
        }
    }

    fn pick_next(&mut self) -> Option<ProcId> {
        if self.ready_queue.is_empty() {
            return None;
        }

        // 选择虚拟运行时间最小的进程
        let mut min_idx = 0;
        let mut min_vruntime = self.get_vruntime(self.ready_queue[0]);

        for (i, &id) in self.ready_queue.iter().enumerate().skip(1) {
            let vruntime = self.get_vruntime(id);
            if vruntime < min_vruntime {
                min_vruntime = vruntime;
                min_idx = i;
            }
        }

        let id = self.ready_queue.remove(min_idx);
        self.stats.context_switches += 1;

        // 新进程的虚拟运行时间增加一个时间单位
        self.update_vruntime(id, 1);

        Some(id)
    }

    fn on_tick(&mut self, _current: Option<ProcId>, _current_time: usize) {
        // CFS通过虚拟运行时间管理，不需要强制时间片
    }

    fn on_block(&mut self, _id: ProcId, _current_time: usize) {}

    fn on_wakeup(&mut self, id: ProcId, _current_time: usize) {
        self.enqueue(id);
    }

    fn get_stats(&self) -> &SchedulerStats {
        &self.stats
    }

    fn get_stats_mut(&mut self) -> &mut SchedulerStats {
        &mut self.stats
    }

    fn is_empty(&self) -> bool {
        self.ready_queue.is_empty()
    }

    fn queue_len(&self) -> usize {
        self.ready_queue.len()
    }
}

/// 调度器类型枚举
#[derive(Clone, Copy, Debug)]
pub enum SchedulerType {
    FCFS,
    SJF,
    RR(usize), // 时间片大小
    MLFQ(usize), // 优先级级数
    CFS,
}

/// 创建指定类型的调度器
pub fn create_scheduler(sched_type: SchedulerType) -> Box<dyn Scheduler> {
    match sched_type {
        SchedulerType::FCFS => Box::new(FCFSScheduler::new()),
        SchedulerType::SJF => Box::new(SJFScheduler::new()),
        SchedulerType::RR(time_slice) => Box::new(RRScheduler::new(time_slice)),
        SchedulerType::MLFQ(levels) => Box::new(MLFQScheduler::new(levels)),
        SchedulerType::CFS => Box::new(CFSScheduler::new()),
    }
}
