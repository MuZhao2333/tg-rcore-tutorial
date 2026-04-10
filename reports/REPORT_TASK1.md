# Task 1: 基础实验实践报告

## 一、实验概述

Task 1 要求充分利用各类AI工具，完成ch3/ch4/ch5/ch6/ch8这5个基础实验练习。

## 二、与AI合作的实现过程

### 2.1 典型合作场景

**场景1：编译错误排查**

运行测试时经常遇到各种编译错误，我会把错误信息直接复制给AI，让它帮我分析。

**场景2：理解代码逻辑**

当我阅读不熟悉的代码时，会请AI帮我解释。

**场景3：画流程图**

有些抽象概念用文字说不清楚，我会请AI帮忙画图。

**场景4：调试运行时问题**

程序能编译但运行出问题，需要通过大量debug输出来定位问题。

### 2.2 遇到的问题及解决

**问题1：ch3 trace编译 - 模块导入和类型注解**

实现trace后遇到编译错误：

```
error[E0433]: failed to resolve: use of unresolved module or unlinked crate `task`
   --> src/main.rs:325:33
    |
325 |                     let count = task::TaskControlBlock::current()
    |                                 ^^^^ use of unresolved module or unlinked crate `task`

error[E0282]: type annotations needed
   --> src/task.rs:134:13
    |
134 |         let id = self.ctx.a(7).into();
    |             ^^
    |                       type must be known at this point
```

我：这些编译错误怎么解决？

AI：需要先`mod task`声明模块，还需要给类型注解...

在AI的指导下，我逐个修复了这些编译问题。

**问题2：ch3 unsafe代码警告**

虽然不是错误，但有很多Unsafe相关的警告：

```
warning[E0133]: use of inline assembly is unsafe and requires unsafe block
   --> /mnt/d/OS/tg-rcore-tutorial-kernel-context/src/lib.rs:157:13

warning[E0133]: dereference of raw pointer is unsafe and requires unsafe block
   --> /mnt/d/OS/tg-rcore-tutorial-kernel-context/src/lib.rs:178:13
```

AI解释说这是Rust 2024 edition的新要求，unsafe代码需要显式标记。

**问题3：ch4 Portal机制理解**

这是ch4的核心内容。我向AI提出了很多问题：

我：ch4的portal是否就等价于ch3的execute_naked里面的那一段空间？

AI：（读取了main.rs）不完全是，portal允许跨地址空间切换...

我：什么是多槽支持？

AI：（读取了process.rs）多槽支持允许同时维护多个跨地址空间的跳转点...

我：portal相比ch3的execute naked增加了什么东西？为什么是必要的？

AI：（读取了notes/ch4.md）关键是需要保存和恢复satp寄存器来切换页表...

**问题4：ch4 trap过程图画图**

我：我画了ch3的trap过程图，现在你阅读ch4的代码，把ch4的trap过程图画出来，并告诉我和ch3有什么不同。

AI：（搜索了execute_naked、SAVE_ALL、LOAD_ALL等相关代码）...

经过多次迭代，AI帮我画出了ch4的trap过程图，清晰地展示了与ch3的差异。

**问题5：ch6 inode方法找不到**

这是ch6文件系统部分的问题：

```
error[E0599]: no method named `inode_id` found for struct `Arc`
  --> src\fs.rs:96:34
   |
96 |             let inode_id = inode.inode_id();

error[E0599]: no method named `link` found for struct `Inode`
  --> src\fs.rs:97:23
```

我：在WSL下明明是正常的，为什么Windows下报错？

AI：（读取了Cargo.toml）让我检查一下依赖版本...

我：rustc和cargo版本都一样啊。

AI：（搜索了Inode相关的实现）可能是easy-fs库的版本不一致...

**问题6：ch6 Git跨平台同步**

这个问题困扰了我很久：

我：为什么vscode显示commit了的change，在wsl下就全都没commit？

AI：（读取了fs.rs）这是因为Windows和WSL实际上是两个不同的文件系统，虽然共享同一个文件夹路径，但Git仓库是独立的...

我终于明白了：Windows下的Git和WSL下的Git是独立的，虽然文件夹路径看起来一样。

**问题7：ch8 unsafe代码警告**

ch8涉及信号和线程机制，编译时出现了大量unsafe相关的警告：

```
warning[E0133]: call to unsafe function `foreign::PortalText::copy_to` is unsafe and requires unsafe block
  --> /mnt/d/OS/tg-rcore-tutorial-kernel-context/src/foreign/multislot_portal.rs:51:9

warning[E0133]: dereference of raw pointer is unsafe and requires unsafe block
  --> /mnt/d/OS/tg-rcore-tutorial-kernel-context/src/foreign/mod.rs:54:24

warning[E0133]: call to unsafe function `foreign::ForeignPortal::transit_entry` is unsafe and requires unsafe block
```

我：为什么ch8有这么多unsafe警告？

AI：这是因为ch8需要处理跨地址空间的上下文切换，涉及到底层的指针操作。Rust要求这些操作必须在unsafe块中进行。

我：这些unsafe代码安全吗？

AI：这些代码本身是安全的，因为有完善的边界检查和状态验证。只是Rust的类型系统无法完全验证这些操作，所以需要标记为unsafe。但调用者需要确保前置条件满足。

**问题8：ch8 crate找不到**

编译ch8时遇到crate依赖问题：

```
error[E0463]: can't find crate for `tg_rcore_tutorial_kernel_context`
  --> src/process.rs:33:5
   |
33 | use tg_kernel_context::{foreign::ForeignContext, L...`
   |     ^^^^^^^^^^^^^^^^^ can't find crate

error[E0463]: can't find crate for `tg_rcore_tutorial_kernel_vm`
  --> src/process.rs:34:5
   |
34 | use tg_kernel_vm::{`
   |     ^^^^^^^^^^^^ can't find crate
```

我：这些crate找不到是什么问题？

AI：这可能是workspace配置问题。让我检查一下Cargo.toml...

AI帮我分析了workspace的依赖结构，发现是tg-kernel系列crate的路径配置问题。

### 2.3 理解核心概念

**理解Trap机制**

通过与AI的多次讨论，我逐渐理解了Trap的完整流程：
1. 用户态程序执行ecall触发trap
2. 硬件自动保存部分寄存器，跳转到stvec指向的地址
3. 内核代码保存完整的上下文
4. 处理系统调用
5. 恢复上下文，通过sret返回

**理解虚拟内存**

我：ch4的portal和ch3的execute_naked有什么区别？

AI：ch3只能跳转到同一个地址空间的函数，ch4的portal允许跨地址空间切换。关键是需要保存和恢复satp寄存器来切换页表。

**理解多槽Portal**

我：什么是多槽支持？

AI：多槽支持允许同时维护多个跨地址空间的跳转点，每个槽可以配置不同的目标地址空间。这在处理复杂的进程切换场景时很有用。

## 三、学习效果评估

### 3.1 知识掌握情况

| 章节 | 核心知识点 | 掌握程度 |
|------|------------|----------|
| ch3 | 系统调用、Trap处理、任务切换 | 能画出流程图 |
| ch4 | Sv39虚拟内存、页表、Portal | 理解地址空间切换机制 |
| ch5 | 进程管理、fork/exec/waitpid | 理解进程生命周期 |
| ch6 | 文件系统、easy-fs、inode | 理解文件操作抽象 |
| ch8 | 线程、同步原语、信号量 | 理解并发控制 |

### 3.2 能力提升

| 能力 | 提升情况 |
|------|----------|
| 代码阅读 | 能够快速定位关键代码，理解模块职责 |
| 问题排查 | 学会将错误信息提供给AI进行分析 |
| 概念理解 | 能够用图示方式表达抽象概念 |
| 跨平台调试 | 了解Windows/WSL环境的差异 |

### 3.3 与本校现有教学对比

| 对比项 |本校实验 | AI辅助实验 |
|--------|----------|------------|
| 学习时间 | 较长 | 大量缩短 |
| 理解深度 | 较深（自己摸索） | 适中（AI辅助理解） |
| 问题解决 | 容易卡住 | 较快找到方向 |
| 主动性 | 必须主动 | 需警惕依赖 |
