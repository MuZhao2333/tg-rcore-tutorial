# Task 2: 个性化实验教程设计报告

## 一、设计思路与目标

根据要求，我选择了两个个性化实验：

**实验1：Lec2 - 时钟中断扩展**

扩展tg-rcore-tutorial-ch1内核crate支持时钟中断，形成一个独立的时钟中断实验。参考ch3中的中断处理机制，理解RISC-V的中断向量和状态寄存器配置。

**实验2：调度算法实验套件**

从"一个scheduler"变成"可插拔scheduler"，实现FCFS/SJF/RR/MLFQ/CFS等多种调度算法，并进行量化比较。

## 二、与AI合作的实现过程

### 2.1 时钟中断实验

**问题1：程序卡在等待中断**

刚开始调试时钟中断时，遇到了程序卡住不动的问题：

```
Init...
sstatus: 0x2
sie: 0x20
time: 0x63e6
mtimecmp: 0x98fa66
Wait for interrupt...
Timer! 0x98fa66
```

我：为什么卡住了？是不是mtimecmp设置成固定值了？

AI：让我检查一下你的代码...问题在于mtimecmp应该设置的是相对时间，而不是绝对时间。每次应该基于当前时间加上一个间隔值。

**问题2：改用真实中断**

我：改用真实中断

AI：（搜索了tg-rcore-tutorial-sbi相关代码）...

**问题3：unclosed delimiter语法错误**

修复中断代码时遇到语法错误：

```
error: this file contains an unclosed delimiter
   --> src/main.rs:309:3
    |
149 | fn print_hex(val: usize) {
    |                          - unclosed delimiter
309 | }
    |  ^ unexpected closing delimiter
```

AI：这通常是因为某个函数或代码块的括号不匹配。让我帮你检查...

**问题4：extern blocks must be unsafe**

```
error: extern blocks must be unsafe
   --> src/main.rs:159:5
    |
159 |     extern "C" {
    |     _________^ extern blocks must be unsafe
```

AI：这是Rust 2024 edition的要求，extern块必须标记为unsafe。

**问题5：function_casts_as_integer错误**

```
error: direct cast of function item into an integer
   --> src/main.rs:126:28
    |
126 |         stvec::write(_trap as usize, stvec::TrapMode::Direct);
    |                            ^^^^^^^^
```

AI：需要先将函数指针转为`void`指针，再转为整数：
```rust
stvec::write(_trap as *const () as usize, stvec::TrapMode::Direct);
```

**问题6：程序仍然卡住**

修复了编译错误后，程序仍然卡住：

```
stvec set, entering interrupt-driven mode...
卡在这
```

AI：让我查看msbi.rs和汇编入口代码...问题可能在于中断向量表的设置或者中断使能位配置。

### 2.2 调度算法实验

**问题1：模块路径问题**

我：调度器代码应该怎么组织？

AI：建议采用trait接口设计，定义统一的调度器接口。每个具体调度算法实现这个接口。

```
pub trait Scheduler {
    fn enqueue(&mut self, task: TaskId);
    fn pick_next(&mut self) -> Option<TaskId>;
    fn on_tick(&mut self);
    fn on_block(&mut self, task: TaskId);
    fn on_wakeup(&mut self, task: TaskId);
}
```

**问题2：编译feature问题**

我：现在是可以在compile的时候指定不同的scheduler吗？

AI：可以通过Rust的feature机制来实现：
```toml
[features]
scheduler-fcfs = []
scheduler-sjf = []
scheduler-rr = []
```

**问题3：Python测试脚本问题**

运行compare_schedulers.py时：

```
❌ FCFS 编译失败
任务数: 0
总耗时: 0 ms
```

我：这对吗？显示一下原始输出，是不是每次build之前没有清理之前的文件？

AI：（读取了processor.rs）问题可能是测试脚本没有正确捕获编译输出，或者scheduler的feature没有正确启用...

**问题4：Rust高级特性编译错误**

项目中涉及大量Rust高级特性，遇到了很多编译错误：

```
error[E0599]: no method named `partial_cmp` found for enum `SchedulerType`
```

AI帮我逐个修复：
- 为SchedulerType实现PartialOrd和Ord trait
- 解决Clone for Box的重复实现冲突
- 处理借用检查器错误
- 添加必要的类型注解

**最终实现**

在AI的协助下，完成了调度器框架的搭建：

```
╔════════════════════════════════════════════════════╗
║     调度算法实验套件 (Scheduling Benchmark Suite) ║
╚════════════════════════════════════════════════════╝

调度器已集成到内核中，支持以下调度算法:
✓ FCFS  - 先进先出
✓ SJF   - 最短作业优先
✓ RR    - 时间片轮转
✓ MLFQ  - 多级反馈队列
✓ CFS   - 完全公平调度
```

## 三、学习效果评估

### 3.1 知识提升

| 方面 | 提升 | 说明 |
|------|------|------|
| 中断机制 | 深入理解 | 掌握RISC-V中断向量、状态寄存器配置 |
| 调度算法 | 系统掌握 | 理解多种调度算法的原理和适用场景 |

### 3.2 与传统教学对比

| 对比项 | 传统方式 | AI辅助方式 |
|--------|----------|------------|
| 问题解决时间 | 较长 | 较短 |
| 理解深度 | 较深 | 适中 |
| 学习主动性 | 必须主动 | 需警惕依赖 |

## 四、实验成果

| 实验名称 | 状态 | 说明 |
|----------|------|------|
| Lec2-时钟中断 | 完成 | ch1添加真实时钟中断支持 |
| Lec4-调度算法实验 | 完成 | 可插拔调度器框架 |
