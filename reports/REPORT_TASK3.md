# Task 3: 扩展实验实践报告

## 一、实验概述

Task 3 要求设计并实现游戏应用，包括至少3个与老师沟通认可的游戏实验。根据要求，我选择了：
- Ch1: Tangram七巧板渲染
- Ch2: Tangram依次渲染
- Ch8: DOOM游戏

## 二、Ch1 Tangram七巧板实验

### 2.1 实验目标

基于ch1的GPU显示功能，用VirtIO GPU渲染"O/S"图案的七巧板图形。

### 2.2 与AI的交互过程

**问题1：GPU设备参数错误**

运行后没有任何反应，没有窗口弹出。

```
qemu-system-riscv64: -device ramfb,addr=0x10000000,width=800,height=600,bpp=32: Property 'ramfb.height' not found
```

我：为什么ramfb参数报错？

AI：看起来QEMU版本不支持这些参数。让我帮你查一下ramfb支持的参数...

**问题2：需要使用virtio-gpu-device**

我：通过qemu-system-riscv64 -device help | grep gpu 查看可用设备

AI：需要使用 `-device virtio-gpu-device` 而不是 ramfb。

**问题3：Rust类型不匹配**

编译时遇到Rust类型错误：

```
error[E0308]: mismatched types
   --> src/main.rs:125:31
    |
125 |     let x2 = (x + w) as i32 + skew;
    |                               ^^^^ expected `i32`, found `isize`
```

AI：这是Rust的类型系统要求，需要显式转换。让我帮你检查一下所有的类型注解...

**问题4：图像横向重复**

好不容易显示出来了，但是七巧板横向重复了4次。

我：为什么现在图形会横向重复4次？

AI：这可能是framebuffer的配置问题，或者显存地址计算有误。需要检查填充函数的边界条件...

**问题5：图案形状不对**

七巧板的形状与目标"O/S"图案不一致。

AI：需要调整七巧板各个组成部分的坐标和旋转角度。七巧板由7个基本图形组成：5个三角形、1个正方形、1个平行四边形。需要精确计算每个图形的顶点位置。

### 2.3 调试过程记录

这个实验经历了多个版本的调试：

1. 初始版本：GPU找不到设备（ramfb参数不支持）
2. 改用virtio-gpu-device后：类型不匹配
3. 修复类型后：能显示但横向重复
4. 调整显存配置后：图像基本正确
5. 手动计算并微调坐标后：最终达到目标效果

## 三、Ch2 Tangram依次渲染实验

### 3.1 实验目标

在ch2中实现七巧板的逐步显示效果——每执行一个app显示七巧板的一个部分。

### 3.2 与AI的交互过程

**问题1：IllegalInstruction异常**

app全部因为IllegalInstruction被杀死：

```
[ERROR] app0 was killed because of Exception(IllegalInstruction)
[ERROR] app1 was killed because of Exception(IllegalInstruction)
...
```

我：这是为何？

AI：这个问题可能是因为用户程序的加载地址或入口点配置不对...

**问题2：app0结束后卡住**

app0结束后程序就卡住了，屏幕显示灰色，没有继续运行。

我：app0结束了就卡住了；屏幕上只是显示灰色；没有再往下运行了

AI：（搜索了app exit相关代码）这个问题可能是因为进程退出后没有正确调度下一个任务...

**问题3：输出全在串口里**

我：现在输出全是在串口里，你看是不是要在config.toml里加上把串口输出到stdio的配置。

AI：ch2的显示可能需要配置nographic...

**问题4：程序没有正确往下进行**

我：不对啊，画面不应该变吗？不应该继续往下执行程序吗？

AI：（读取了main.rs）需要检查batch processing的逻辑...

**问题5：APP0 START后无后续输出**

```
>>> APP0 START >>
并没有退出；你看看输出，现在是有什么问题？
```

AI：（读取了gpu.rs）问题可能在于GPU初始化后没有正确切换显示模式...

**问题6：显存地址不匹配**

我反复调试，发现是显存地址配置问题导致显示失败。

AI：让我看看你的framebuffer地址配置...需要确保GPU看到的地址和CPU看到的一致。

### 3.3 调试过程

这个实验花费了大量时间，经历了多次调试：

1. 首先修复了app加载地址问题
2. 然后解决GPU显示初始化问题
3. 接着处理七巧板绘制的坐标问题
4. 最后优化显示效果的细节

## 四、Ch8 DOOM游戏实验

### 4.1 实验目标

基于rcore-tutorial-ch8，实现用户态DOOM游戏，使用doomgeneric引擎。

### 4.2 与AI的交互过程

**问题1：ch8大量crate找不到**

这是ch8最开始的编译错误，涉及很多crate依赖问题：

```
error[E0463]: can't find crate for `tg_rcore_tutorial_kernel_context`
error[E0463]: can't find crate for `tg_rcore_tutorial_kernel_vm`
error[E0463]: can't find crate for `tg_rcore_tutorial_kernel_alloc`
error[E0463]: can't find crate for `tg_rcore_tutorial_easy_fs`
error[E0463]: can't find crate for `tg_rcore_tutorial_sync`
error[E0463]: can't find crate for `riscv`
```

我：这些crate找不到是什么问题？

AI：这可能是workspace配置问题或者依赖路径问题。让我检查一下Cargo.toml...

**问题2：Sv39配置问题**

```
error[E0432]: unresolved import `crate::Sv39`
  --> src/process.rs:27:78
   |
27 | use crate::{processor::ProcessorInner, Sv39, Sv39Manager, ...}
   |              ^^^^ no `Sv39` in the root
   |
note: found an item that was configured out
   --> src/main.rs:80:11
   |
79 | #[cfg(not(target_arch = "riscv64"))]
   |          ------------------------- the item is gated here
80 | use stub::Sv39;
```

AI：Sv39只在riscv64架构下可用。在其他平台上被配置掉了。

**问题3：大量trait找不到**

```
error[E0405]: cannot find trait `IO` in this scope
error[E0405]: cannot find trait `Process` in this scope
error[E0405]: cannot find trait `Scheduling` in this scope
error[E0405]: cannot find trait `Clock` in this scope
error[E0405]: cannot find trait `Signal` in this scope
error[E0425]: cannot find type `Caller` in this scope
```

我：为什么这么多trait找不到？

AI：这些trait定义在tg-syscall crate中。需要确保Cargo.toml中正确引入依赖。

**问题4：fs.img文件被锁定**

编译成功后运行时报错：

```
qemu-system-riscv64: -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0: Failed to get "write" lock
Is another process using the image [target/riscv64gc-unknown-none-elf/debug/fs.img]?
```

我：告诉我解锁的命令

AI：可能是QEMU进程没有正确退出。可以尝试：
```bash
killall qemu-system-riscv64
```
或者直接删除锁文件。

**问题5：GPU初始化后卡住**

运行输出显示GPU初始化成功，但之后卡住不动：

```
[ INFO] GPU initialization (VirtIO mode)
[GPU] init_gpu() start (VirtIO mode)
[GPU] Found VirtIO GPU at MMIO base: 0x10008000
[ INFO] Framebuffer: ptr=0x8026f000, len=3145728, 1024x768
卡在这了
```

我：卡在这了

AI：需要增加更多调试输出来定位问题位置。问题可能在于doom_stub进程启动或页面映射。

**问题6：StorePageFault异常**

运行时遇到页面错误：

```
[ERROR] unsupported trap: Exception(StorePageFault)
>> root@LAPTOP-MVSJ880P:/mnt/d/OS/tg-rcore-tutorial/tg-rcore-tutorial-ch8#
```

AI：这是因为framebuffer地址没有被正确映射到用户态页表。需要确保在用户进程创建时将framebuffer区域映射进去。

**问题7：页表映射断言失败**

运行时panic：

```
panicked at /mnt/d/OS/tg-rcore-tutorial/tg-rcore-tutorial-kernel-vm/src/space/mapper.rs:37:9:
assertion failed: !pte.is_valid()
```

AI：断言失败说明在映射页面时发现目标页表项已经被占用。需要检查是否重复映射了同一块内存区域。

**问题8：doom用户程序sbrk函数找不到**

```
error[E0425]: cannot find function `sbrk` in this scope
  --> src/doomgeneric.rs:61:23
   |
61 |             let ptr = sbrk(buffer_size as i32) as *mut u8;
   |                       ^^^^ not found in this scope
```

我：doomgeneric需要sbrk来分配内存

AI：需要在user_lib中添加sbrk系统调用的绑定。

**问题9：用户程序mutable static警告**

用户态代码中大量使用mutable static：

```
warning[E0133]: use of mutable static is unsafe and requires unsafe block
  --> src/bin/mpsc_sem.rs:45:30
   |
45 |         print!("{} ", BUFFER[TAIL]);
   |                              ^^^^ use of mutable static
```

AI：在嵌入式/OS环境中使用mutable static需要格外小心，需要在unsafe块中进行。

**问题10：volatile读写警告**

```
warning[E0133]: call to unsafe function `core::ptr::mut_ptr::::read_volatile` is unsafe
warning[E0133]: call to unsafe function `core::ptr::mut_ptr::::write_volatile` is unsafe
```

AI：volatile读写操作本身是不安全的，但在这个场景下是正确的用法（硬件寄存器访问）。

**问题11：build.rs中block不完整错误**

```
thread 'main' (212706) panicked at build.rs:150:9:
assertion `left == right` failed: Not a complete block!
  left: 0
  right: 512
```

AI：这是因为用户程序镜像没有512字节对齐。需要调整build.rs中的镜像打包逻辑。

### 4.3 成功运行

经过多次调试和修复，最终成功运行：

```
[DEBUG] doom: starting...
[DEBUG] doom: framebuffer check...
doom: framebuffer 1024x768
[DEBUG] doom: checking doom1.wad...
doom: found doom1.wad (fd=3)
doom: starting DOOM engine...
Doom Generic 0.1
zone memory: 0xb98b0, 600000 allocated for zone
===========================================================================
DOOM Shareware
===========================================================================
```

## 五、学习效果评估

### 5.1 知识提升

| 知识点 | 掌握程度 | 说明 |
|--------|----------|------|
| GPU驱动 | 理解 | VirtIO GPU的工作原理和编程接口 |
| 图形渲染 | 理解 | 三角形光栅化、颜色填充 |
| 游戏引擎移植 | 了解 | 跨平台游戏引擎的移植要点 |
| 内核扩展 | 掌握 | 如何扩展操作系统内核功能 |
| 页表映射 | 深入 | 理解用户态地址空间映射 |

### 5.2 能力提升

- **调试能力**：面对复杂的图形显示问题，能够系统性地排查
- **阅读代码能力**：能够快速理解doomgeneric等大型项目的结构
- **问题解决能力**：从多个角度尝试解决问题
- **跨模块调试**：涉及内核、驱动、用户程序的综合调试

### 5.3 与传统学习方式对比

| 对比项 | 传统学习 | 扩展实验 |
|--------|----------|----------|
| 学习动力 | 一般 | 较强（做游戏有趣） |
| 知识深度 | 较浅 | 较深（需要实现具体功能） |
| 成就感 | 一般 | 较强（能运行游戏） |
