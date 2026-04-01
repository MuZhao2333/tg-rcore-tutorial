use super::UPIntrFreeCell;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use tg_task_manage::ThreadId;

// 教程说明：
// `count` 含义采用经典信号量语义：
// - count >= 0：可用资源数；
// - count < 0：有 `-count` 个线程在等待队列中。

/// Semaphore
pub struct Semaphore {
    /// UPIntrFreeCell<SemaphoreInner>
    pub inner: UPIntrFreeCell<SemaphoreInner>,
}

/// SemaphoreInner
pub struct SemaphoreInner {
    pub count: isize,
    /// total = initial resource count (constant)
    pub total: usize,
    /// allocation map: (ThreadId, allocated_count)
    pub alloc_map: Vec<(ThreadId, usize)>,
    pub wait_queue: VecDeque<ThreadId>,
}

impl Semaphore {
    /// 创建一个新的信号量，初始资源计数为 `res_count`。
    pub fn new(res_count: usize) -> Self {
        Self {
            // SAFETY: 此信号量仅在单处理器内核环境中使用
            inner: unsafe {
                UPIntrFreeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    total: res_count,
                    alloc_map: Vec::new(),
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
    /// 当前线程释放信号量表示的一个资源，并唤醒一个阻塞的线程
    pub fn up(&self, tid: ThreadId) -> Option<ThreadId> {
        let mut inner = self.inner.exclusive_access();
        // 当前线程释放一个已分配的资源（如果有的话）
        if let Some(pos) = inner.alloc_map.iter().position(|(t, _)| *t == tid) {
            if inner.alloc_map[pos].1 > 1 {
                inner.alloc_map[pos].1 -= 1;
            } else {
                inner.alloc_map.swap_remove(pos);
            }
        }
        inner.count += 1;
        // 若有等待者，交由调度器唤醒队首线程，并把资源计入唤醒线程的分配。
        if let Some(waking) = inner.wait_queue.pop_front() {
            // 增加唤醒线程的 allocation 计数
            if let Some(pos) = inner.alloc_map.iter().position(|(t, _)| *t == waking) {
                inner.alloc_map[pos].1 += 1;
            } else {
                inner.alloc_map.push((waking, 1));
            }
            Some(waking)
        } else {
            None
        }
    }
    /// 当前线程试图获取信号量表示的资源，并返回结果
    pub fn down(&self, tid: ThreadId) -> bool {
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            // 资源不足：当前线程进入等待队列。
            inner.wait_queue.push_back(tid);
            drop(inner);
            false
        } else {
            // 成功获取：记录分配给该线程的数量
            if let Some(pos) = inner.alloc_map.iter().position(|(t, _)| *t == tid) {
                inner.alloc_map[pos].1 += 1;
            } else {
                inner.alloc_map.push((tid, 1));
            }
            true
        }
    }
}
