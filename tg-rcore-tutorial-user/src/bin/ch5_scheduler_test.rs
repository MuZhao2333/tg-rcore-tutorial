#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::{println, getpid, fork, wait, get_time};

// 任务性能统计结构
#[derive(Clone, Copy)]
struct TaskMetrics {
    pid: usize,
    start_time: isize,
    end_time: isize,
    task_type: u32, // 0=CPU, 1=IO, 2=Interactive
}

// 全局统计存储（假设最多50个任务）
static mut METRICS: [TaskMetrics; 50] = [TaskMetrics {
    pid: 0,
    start_time: 0,
    end_time: 0,
    task_type: 0,
}; 50];
static mut METRIC_COUNT: usize = 0;

// CPU密集型任务 (task_type=0)
fn cpu_bound_task(duration: usize, task_id: usize) {
    let my_pid = getpid();
    let start_time = get_time();
    let mut sum: u64 = 0;
    
    println!("[TASK_START] PID={} TYPE=CPU ID={} TIME={}", my_pid, task_id, start_time);
    println!("[SCHED_LOG] T={} EVENT=task_start PID={} TYPE=CPU", start_time, my_pid);
    
    for i in 0..duration * 1000000 {
        sum = sum.wrapping_add(i as u64);
    }
    
    let end_time = get_time();
    println!("[TASK_END] PID={} TYPE=CPU ID={} TIME={} DURATION={}", 
             my_pid, task_id, end_time, end_time - start_time);
    println!("[SCHED_LOG] T={} EVENT=task_end PID={} TYPE=CPU DURATION={}", end_time, my_pid, end_time - start_time);
    println!("[CPU_SUM] {}", sum);
}

// IO密集型任务 (task_type=1)
fn io_bound_task(duration: usize, task_id: usize) {
    let my_pid = getpid();
    let start_time = get_time();
    
    println!("[TASK_START] PID={} TYPE=IO ID={} TIME={}", my_pid, task_id, start_time);
    println!("[SCHED_LOG] T={} EVENT=task_start PID={} TYPE=IO", start_time, my_pid);
    
    user_lib::sleep(duration * 10);
    
    let end_time = get_time();
    println!("[TASK_END] PID={} TYPE=IO ID={} TIME={} DURATION={}", 
             my_pid, task_id, end_time, end_time - start_time);
    println!("[SCHED_LOG] T={} EVENT=task_end PID={} TYPE=IO DURATION={}", end_time, my_pid, end_time - start_time);
}

// 交互式任务 (task_type=2)
fn interactive_task(iterations: usize, task_id: usize) {
    let my_pid = getpid();
    let start_time = get_time();
    
    println!("[TASK_START] PID={} TYPE=INTER ID={} TIME={}", my_pid, task_id, start_time);
    println!("[SCHED_LOG] T={} EVENT=task_start PID={} TYPE=INTER", start_time, my_pid);
    
    for i in 0..iterations {
        let mut sum: u64 = 0;
        for j in 0..100000 {
            sum = sum.wrapping_add(j as u64);
        }
        if i % 5 == 0 {
            user_lib::sleep(1);
        }
    }
    
    let end_time = get_time();
    println!("[TASK_END] PID={} TYPE=INTER ID={} TIME={} DURATION={}", 
             my_pid, task_id, end_time, end_time - start_time);
    println!("[SCHED_LOG] T={} EVENT=task_end PID={} TYPE=INTER DURATION={}", end_time, my_pid, end_time - start_time);
}

#[unsafe(no_mangle)]
extern "C" fn main() -> i32 {
    let global_start = get_time();
    
    println!("\n╔════════════════════════════════════════╗");
    println!("║   Scheduler Benchmark Suite (Ch5)     ║");
    println!("║       Global Start: {}ms", global_start);
    println!("╚════════════════════════════════════════╝");
    println!("[WORKLOAD_START] TIME={}", global_start);
    
    // Workload 1: CPU-Bound Tasks
    println!("\n[WORKLOAD] TYPE=CPU-BOUND");
    for i in 0..3 {
        let pid = fork();
        if pid == 0 {
            cpu_bound_task(1, i);
            return 0;
        }
    }
    
    let mut exit_code: i32 = 0;
    for _ in 0..3 {
        wait(&mut exit_code);
    }
    println!("[WORKLOAD_COMPLETE] TYPE=CPU-BOUND");

    // Workload 2: IO-Bound Tasks
    println!("\n[WORKLOAD] TYPE=IO-BOUND");
    for i in 0..4 {
        let pid = fork();
        if pid == 0 {
            io_bound_task(1, i);
            return 0;
        }
    }

    for _ in 0..4 {
        wait(&mut exit_code);
    }
    println!("[WORKLOAD_COMPLETE] TYPE=IO-BOUND");

    // Workload 3: Interactive Tasks
    println!("\n[WORKLOAD] TYPE=INTERACTIVE");
    for i in 0..2 {
        let pid = fork();
        if pid == 0 {
            interactive_task(20, i);
            return 0;
        }
    }

    for _ in 0..2 {
        wait(&mut exit_code);
    }
    println!("[WORKLOAD_COMPLETE] TYPE=INTERACTIVE");

    // Workload 4: Mixed High-Concurrency
    println!("\n[WORKLOAD] TYPE=MIXED");
    for i in 0..6 {
        let pid = fork();
        if pid == 0 {
            match i % 3 {
                0 => cpu_bound_task(1, i),
                1 => io_bound_task(1, i),
                _ => interactive_task(10, i),
            }
            return 0;
        }
    }

    for _ in 0..6 {
        wait(&mut exit_code);
    }
    println!("[WORKLOAD_COMPLETE] TYPE=MIXED");

    let global_end = get_time();
    println!("\n╔════════════════════════════════════════╗");
    println!("║      All Benchmarks Completed         ║");
    println!("║       Global End: {}ms", global_end);
    println!("║      Total Time: {}ms", global_end - global_start);
    println!("╚════════════════════════════════════════╝");
    println!("[WORKLOAD_END] TIME={} TOTAL={}", global_end, global_end - global_start);
    
    0
}
