//! VirtIO GPU 驱动模块
//!
//! 完整的VirtIO GPU MMIO驱动实现，支持QEMU虚拟化GPU设备
//! 通过virtio-drivers库进行MMIO操作
//!
//! 移植自 tg-rcore-tutorial-ch2，针对 ch8 适配

use core::sync::atomic::{AtomicUsize, Ordering};
use core::sync::atomic::AtomicBool;
use core::mem::MaybeUninit;
use core::cell::UnsafeCell;
use virtio_drivers::{VirtIOGpu, VirtIOHeader, MmioTransport};
use tg_console::log;

/// Framebuffer 信息结构（供用户程序通过 syscall 获取）
#[repr(C)]
pub struct FbInfo {
    pub ptr: usize,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: u32,
}

// Wrapper 用于在静态上下文中安全地保存 UnsafeCell，因为静态需要 `Sync`。
struct GpuStorage(UnsafeCell<MaybeUninit<VirtIOGpu<'static, SimpleHal, MmioTransport>>>);
unsafe impl Sync for GpuStorage {}

/// 常量定义
#[cfg(target_arch = "riscv64")]
const VIRTIO_MMIO_BASE: usize = 0x1000_2000; // Slot 1 = GPU（Slot 0 被块设备占用）
#[cfg(target_arch = "riscv64")]
const VIRTIO_MMIO_STRIDE: usize = 0x1000;
#[cfg(target_arch = "riscv64")]
const VIRTIO_MMIO_COUNT: usize = 7; // 从 slot 1 开始，剩余 7 个槽位

const VIRTIO_MAGIC: u32 = 0x7472_6976;  // "virt" in little-endian
const DEVICE_ID_GPU: u32 = 16;          // VirtIO GPU device ID

const FRAMEBUFFER_BACKGROUND: u32 = 0xff1d2128;

/// 用户态映射帧缓冲的固定虚拟基址（页对齐，低于常见 ELF 装载地址 0x8040_0000）。
#[cfg(target_arch = "riscv64")]
pub const USER_FRAMEBUFFER_VA: usize = 0x7000_0000;

/// 显示分辨率（默认值）
const XRES: usize = 1280;
const YRES: usize = 800;

// DMA内存池配置
const DMA_PAGE_SIZE: usize = 4096;
const DMA_POOL_PAGES: usize = 1024;

/// DMA内存池
#[repr(align(4096))]
struct DmaPool([u8; DMA_POOL_PAGES * DMA_PAGE_SIZE]);

static mut DMA_POOL: DmaPool = DmaPool([0; DMA_POOL_PAGES * DMA_PAGE_SIZE]);
static DMA_NEXT_PAGE: AtomicUsize = AtomicUsize::new(0);

/// SimpleHal - 简单硬件抽象层，用于裸机无MMU环境
struct SimpleHal;

impl virtio_drivers::Hal for SimpleHal {
    fn dma_alloc(pages: usize) -> virtio_drivers::PhysAddr {
        if pages == 0 {
            return 0;
        }

        loop {
            let current = DMA_NEXT_PAGE.load(Ordering::Relaxed);
            let next = match current.checked_add(pages) {
                Some(v) => v,
                None => {
                    log::error!("DMA pool exhausted");
                    return 0;
                }
            };

            if next > DMA_POOL_PAGES {
                log::error!("DMA pool exhausted: need {} pages, available {}", pages, DMA_POOL_PAGES - current);
                return 0;
            }

            if DMA_NEXT_PAGE
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                let base = unsafe { core::ptr::addr_of_mut!(DMA_POOL.0) as usize };
                let addr = base + current * DMA_PAGE_SIZE;
                log::debug!("DMA alloc: {} pages at 0x{:x}", pages, addr);
                return addr;
            }
        }
    }

    fn dma_dealloc(_paddr: virtio_drivers::PhysAddr, _pages: usize) -> i32 {
        // 裸机环境通常不释放
        0
    }

    fn phys_to_virt(paddr: virtio_drivers::PhysAddr) -> virtio_drivers::VirtAddr {
        paddr  // 恒等映射
    }

    fn virt_to_phys(vaddr: virtio_drivers::VirtAddr) -> virtio_drivers::PhysAddr {
        vaddr  // 恒等映射
    }
}

/// 查找VirtIO GPU MMIO地址
#[cfg(target_arch = "riscv64")]
fn find_virtio_gpu_mmio() -> Option<usize> {
    for slot in 0..VIRTIO_MMIO_COUNT {
        let base = VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE;
        log::info!("[GPU] Checking MMIO slot {} at 0x{:x}", slot, base);

        // 读取VIRTIO魔数（偏移0x000）
        let magic = unsafe { (base as *const u32).read_volatile() };
        log::info!("[GPU]   magic=0x{:08x} (expected 0x74726976)", magic);

        // 读取设备ID（偏移0x008）
        let device_id = unsafe { ((base + 0x008) as *const u32).read_volatile() };
        log::info!("[GPU]   device_id={} (GPU=16)", device_id);

        if magic == VIRTIO_MAGIC && device_id == DEVICE_ID_GPU {
            log::info!("[GPU] Found VirtIO GPU at MMIO base: 0x{:x}", base);
            return Some(base);
        }
    }

    log::warn!("VirtIO GPU device not found in MMIO space");
    None
}

/// Framebuffer 地址（供用户程序访问）
pub static FRAMEBUFFER_ADDR: AtomicUsize = AtomicUsize::new(0);

/// GPU设备MMIO地址（支持flush操作使用）
static GPU_MMIO_ADDR: AtomicUsize = AtomicUsize::new(0);
static GPU_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 记录 framebuffer 的长度、分辨率与 pitch（字节）
static FRAMEBUFFER_LEN: AtomicUsize = AtomicUsize::new(0);
static FRAMEBUFFER_WIDTH: AtomicUsize = AtomicUsize::new(XRES);
static FRAMEBUFFER_HEIGHT: AtomicUsize = AtomicUsize::new(YRES);
static FRAMEBUFFER_PITCH: AtomicUsize = AtomicUsize::new(XRES * 4);

// 全局保存的 VirtIOGpu 对象（初始化后重用以避免每次 flush 时重建）
static GLOBAL_GPU_CELL: GpuStorage = GpuStorage(UnsafeCell::new(MaybeUninit::uninit()));
static GLOBAL_GPU_INIT: AtomicBool = AtomicBool::new(false);

/// 清空Framebuffer到背景色
fn clear_framebuffer(framebuffer: &mut [u8], len: usize) {
    let pixel = FRAMEBUFFER_BACKGROUND.to_le_bytes();  // BGRA格式
    for chunk in framebuffer[..len].chunks_exact_mut(4) {
        chunk.copy_from_slice(&pixel);
    }
}

/// 初始化GPU设备
/// 返回(framebuffer_ptr, framebuffer_len, width, height)
pub fn init_gpu() -> (usize, usize, usize, usize) {
    log::info!("GPU initialization (VirtIO mode)");

    #[cfg(not(target_arch = "riscv64"))]
    {
        log::warn!("GPU not supported on this architecture");
        const FB_ADDR: usize = 0x8000_0000;
        // 记录回退 framebuffer 信息
        FRAMEBUFFER_ADDR.store(FB_ADDR, Ordering::SeqCst);
        FRAMEBUFFER_LEN.store(1280 * 800 * 4, Ordering::SeqCst);
        FRAMEBUFFER_WIDTH.store(XRES, Ordering::SeqCst);
        FRAMEBUFFER_HEIGHT.store(YRES, Ordering::SeqCst);
        FRAMEBUFFER_PITCH.store(XRES * 4, Ordering::SeqCst);
        return (FB_ADDR, 1280 * 800 * 4, XRES, YRES);
    }

    #[cfg(target_arch = "riscv64")]
    {
        // 1. 查找GPU MMIO地址
        let gpu_mmio = match find_virtio_gpu_mmio() {
            Some(mmio) => mmio,
            None => {
                log::warn!("VirtIO GPU not found, using fallback framebuffer");
                const FB_ADDR: usize = 0x8000_0000;
                // 记录回退 framebuffer 信息
                FRAMEBUFFER_ADDR.store(FB_ADDR, Ordering::SeqCst);
                FRAMEBUFFER_LEN.store(1280 * 800 * 4, Ordering::SeqCst);
                FRAMEBUFFER_WIDTH.store(XRES, Ordering::SeqCst);
                FRAMEBUFFER_HEIGHT.store(YRES, Ordering::SeqCst);
                FRAMEBUFFER_PITCH.store(XRES * 4, Ordering::SeqCst);
                return (FB_ADDR, 1280 * 800 * 4, XRES, YRES);
            }
        };

        // 2. 创建VirtIO GPU驱动
        let transport = unsafe {
            match MmioTransport::new(
                core::ptr::NonNull::new(gpu_mmio as *mut VirtIOHeader).unwrap()
            ) {
                Ok(t) => t,
                Err(e) => {
                    log::error!("Failed to create MmioTransport: {:?}", e);
                    const FB_ADDR: usize = 0x8000_0000;
                    // 记录回退 framebuffer 信息
                    FRAMEBUFFER_ADDR.store(FB_ADDR, Ordering::SeqCst);
                    FRAMEBUFFER_LEN.store(1280 * 800 * 4, Ordering::SeqCst);
                    FRAMEBUFFER_WIDTH.store(XRES, Ordering::SeqCst);
                    FRAMEBUFFER_HEIGHT.store(YRES, Ordering::SeqCst);
                    FRAMEBUFFER_PITCH.store(XRES * 4, Ordering::SeqCst);
                    return (FB_ADDR, 1280 * 800 * 4, XRES, YRES);
                }
            }
        };

        let mut gpu: VirtIOGpu<SimpleHal, MmioTransport> = match VirtIOGpu::new(transport) {
            Ok(g) => g,
            Err(e) => {
                log::error!("Failed to create VirtIOGpu: {:?}", e);
                const FB_ADDR: usize = 0x8000_0000;
                // 记录回退 framebuffer 信息
                FRAMEBUFFER_ADDR.store(FB_ADDR, Ordering::SeqCst);
                FRAMEBUFFER_LEN.store(1280 * 800 * 4, Ordering::SeqCst);
                FRAMEBUFFER_WIDTH.store(XRES, Ordering::SeqCst);
                FRAMEBUFFER_HEIGHT.store(YRES, Ordering::SeqCst);
                FRAMEBUFFER_PITCH.store(XRES * 4, Ordering::SeqCst);
                return (FB_ADDR, 1280 * 800 * 4, XRES, YRES);
            }
        };

        // 3. 查询显示分辨率
        let (width, height) = match gpu.resolution() {
            Ok((w, h)) => (w as usize, h as usize),
            Err(e) => {
                log::error!("Failed to query GPU resolution: {:?}", e);
                (XRES, YRES)  // fallback
            }
        };
        log::info!("GPU resolution: {}x{}", width, height);

        // 4. 设置Framebuffer
        let framebuffer: &mut [u8] = match gpu.setup_framebuffer() {
            Ok(fb) => fb,
            Err(e) => {
                log::error!("Failed to setup framebuffer: {:?}", e);
                const FB_ADDR: usize = 0x8000_0000;
                // 记录回退 framebuffer 信息
                FRAMEBUFFER_ADDR.store(FB_ADDR, Ordering::SeqCst);
                FRAMEBUFFER_LEN.store(1280 * 800 * 4, Ordering::SeqCst);
                FRAMEBUFFER_WIDTH.store(XRES, Ordering::SeqCst);
                FRAMEBUFFER_HEIGHT.store(YRES, Ordering::SeqCst);
                FRAMEBUFFER_PITCH.store(XRES * 4, Ordering::SeqCst);
                return (FB_ADDR, 1280 * 800 * 4, XRES, YRES);
            }
        };

        let framebuffer_ptr = framebuffer.as_mut_ptr() as usize;
        let framebuffer_len = framebuffer.len();

        log::info!(
            "Framebuffer: ptr=0x{:x}, len={}, {}x{}",
            framebuffer_ptr,
            framebuffer_len,
            width,
            height
        );

        // 5. 清空Framebuffer背景
        clear_framebuffer(framebuffer, framebuffer_len);

        // 记录实际 framebuffer 信息：长度、分辨率与 pitch
        FRAMEBUFFER_WIDTH.store(width, Ordering::SeqCst);
        FRAMEBUFFER_HEIGHT.store(height, Ordering::SeqCst);
        FRAMEBUFFER_LEN.store(framebuffer_len, Ordering::SeqCst);
        // 计算 pitch（字节/行），使用 len/height 作为更保守的计算
        let pitch = if height > 0 { framebuffer_len / height } else { width * 4 };
        FRAMEBUFFER_PITCH.store(pitch, Ordering::SeqCst);

        // 6. 将 gpu 对象保存为全局并执行初始 flush 操作，之后在后续 flush 中复用
        unsafe {
            let cell_ptr = GLOBAL_GPU_CELL.0.get();
            let gpu_ptr = (*cell_ptr).as_mut_ptr();
            core::ptr::write(gpu_ptr, gpu);
            GLOBAL_GPU_INIT.store(true, Ordering::SeqCst);
            // move the stored object back into a local to call flush (avoids taking &mut static)
            let gpu_val = core::ptr::read(gpu_ptr);
            let mut gpu_owned = gpu_val;
            match (&mut gpu_owned).flush() {
                Ok(()) => {
                    log::info!("GPU initialized and flushed successfully");
                    GPU_MMIO_ADDR.store(gpu_mmio, Ordering::SeqCst);
                    GPU_INITIALIZED.store(true, Ordering::SeqCst);
                }
                Err(e) => {
                    log::error!("Initial GPU flush failed: {:?}", e);
                }
            }
            // write it back to the global storage
            core::ptr::write(gpu_ptr, gpu_owned);
        }

        // 7. 记录framebuffer地址供用户程序访问
        FRAMEBUFFER_ADDR.store(framebuffer_ptr, Ordering::SeqCst);

        (framebuffer_ptr, framebuffer_len, width, height)
    }
}

/// 获取framebuffer指针
pub fn get_framebuffer_ptr() -> usize {
    FRAMEBUFFER_ADDR.load(Ordering::SeqCst)
}

/// 帧缓冲字节长度（与 VirtIO 分配的 `framebuffer` 一致）
pub fn get_framebuffer_len() -> usize {
    FRAMEBUFFER_LEN.load(Ordering::SeqCst)
}

/// 供 `get_fb_info` 写入用户结构体：必须与 `process::map_user_framebuffer` 映射一致。
#[cfg(target_arch = "riscv64")]
pub fn user_framebuffer_ptr() -> usize {
    let pa = FRAMEBUFFER_ADDR.load(Ordering::SeqCst);
    if pa == 0 {
        return 0;
    }
    const PS: usize = 4096;
    let page_off = pa & (PS - 1);
    USER_FRAMEBUFFER_VA + page_off
}

#[cfg(not(target_arch = "riscv64"))]
pub fn user_framebuffer_ptr() -> usize {
    FRAMEBUFFER_ADDR.load(Ordering::SeqCst)
}

/// 获取framebuffer pitch（字节/行）
pub fn get_framebuffer_pitch() -> usize {
    FRAMEBUFFER_PITCH.load(Ordering::SeqCst)
}

/// 获取分辨率（返回实时存储的值）
pub fn get_resolution() -> (u32, u32) {
    (
        FRAMEBUFFER_WIDTH.load(Ordering::SeqCst) as u32,
        FRAMEBUFFER_HEIGHT.load(Ordering::SeqCst) as u32,
    )
}

/// 刷新GPU显示
pub fn gpu_flush() -> isize {
    if !GPU_INITIALIZED.load(Ordering::SeqCst) {
        log::debug!("[GPU] Flush called but GPU not initialized");
        return 0;  // GPU未初始化，忽略flush
    }

    let gpu_mmio = GPU_MMIO_ADDR.load(Ordering::SeqCst);
    if gpu_mmio == 0 {
        log::debug!("[GPU] Flush called but GPU MMIO address is invalid");
        return 0;
    }

    // log::trace!("[GPU] Framebuffer flush called, MMIO=0x{:x}", gpu_mmio);

    // 优先使用全局保存的 gpu 对象
    if GLOBAL_GPU_INIT.load(Ordering::SeqCst) {
        unsafe {
            let cell_ptr = GLOBAL_GPU_CELL.0.get();
            let gpu_ptr = (*cell_ptr).as_mut_ptr();
            // 将对象读出至栈上临时可变所有权，调用 flush 后再写回
            let mut gpu_owned = core::ptr::read(gpu_ptr);
            match (&mut gpu_owned).flush() {
                Ok(_) => {
                    //log::trace!("[GPU] Flush successful (global gpu)");
                    // 将对象写回全局存储
                    core::ptr::write(gpu_ptr, gpu_owned);
                    return 0;
                }
                Err(e) => {
                    log::debug!("[GPU] Global flush failed: {:?}", e);
                    core::ptr::write(gpu_ptr, gpu_owned);
                    // 继续尝试回退路径
                }
            }
        }
    }

    // 回退：尝试临时创建驱动并 flush
    unsafe {
        if let Ok(transport) = MmioTransport::new(
            core::ptr::NonNull::new(gpu_mmio as *mut VirtIOHeader).unwrap()
        ) {
            if let Ok(mut gpu) = VirtIOGpu::<SimpleHal, MmioTransport>::new(transport) {
                match gpu.flush() {
                    Ok(_) => {
                        log::debug!("[GPU] Flush successful (transient gpu)");
                        return 0;
                    }
                    Err(e) => {
                        log::debug!("[GPU] Flush failed: {:?}", e);
                    }
                }
            } else {
                log::debug!("[GPU] Failed to create VirtIOGpu driver");
            }
        } else {
            log::debug!("[GPU] Failed to create MmioTransport");
        }
    }

    // 如果VirtIO flush失败，至少framebuffer数据已经写入
    log::debug!("[GPU] Flush operation completed (result uncertain)");
    0
}

/// DOOM 离屏缓冲固定为 320×200 调色板索引
pub const DOOM_BLIT_BYTES: usize = 320 * 200;

/// 从内核可访问的切片将 DOOM 调色板帧复制到 GPU 帧缓冲并 flush。
pub fn gpu_blit_from_slice(src: &[u8]) -> isize {
    let fb_ptr = FRAMEBUFFER_ADDR.load(Ordering::SeqCst);
    let fb_pitch = FRAMEBUFFER_PITCH.load(Ordering::SeqCst);
    let fb_width = FRAMEBUFFER_WIDTH.load(Ordering::SeqCst) as usize;
    let fb_height = FRAMEBUFFER_HEIGHT.load(Ordering::SeqCst) as usize;

    if fb_ptr == 0 {
        log::error!("[GPU] Blit failed: framebuffer not initialized");
        return -1;
    }

    let doom_width = 320usize;
    let doom_height = 200usize;
    let doom_size = DOOM_BLIT_BYTES;

    if src.len() < doom_size {
        log::error!(
            "[GPU] Blit: source too small {} < {}",
            src.len(),
            doom_size
        );
        return -1;
    }
    let src = &src[..doom_size];

    let scale_x = (fb_width / doom_width).max(1);
    let scale_y = (fb_height / doom_height).max(1);

    unsafe {
        let dst = core::ptr::slice_from_raw_parts_mut(fb_ptr as *mut u8, fb_height * fb_pitch);

        for y in 0..fb_height {
            let row_base = y * fb_pitch;
            for x in 0..fb_width {
                let idx = row_base + x * 4;
                (*dst)[idx] = 0x28;
                (*dst)[idx + 1] = 0x1d;
                (*dst)[idx + 2] = 0x1d;
                (*dst)[idx + 3] = 0xFF;
            }
        }

        for sy in 0..doom_height {
            for sx in 0..doom_width {
                let color_idx = src[sy * doom_width + sx] as usize;
                let (r, g, b) = doom_palette_expand(color_idx);

                let start_x = sx * scale_x;
                let start_y = sy * scale_y;

                for dy in 0..scale_y {
                    for dx in 0..scale_x {
                        let px = start_x + dx;
                        let py = start_y + dy;
                        if px < fb_width && py < fb_height {
                            let idx = py * fb_pitch + px * 4;
                            (*dst)[idx] = b;
                            (*dst)[idx + 1] = g;
                            (*dst)[idx + 2] = r;
                            (*dst)[idx + 3] = 0xFF;
                        }
                    }
                }
            }
        }
    }

    gpu_flush();
    doom_size as isize
}

/// 从恒等映射下的物理地址读取 DOOM 缓冲（仅当 `src_phys` 在内核可直接解引用时安全）
#[allow(dead_code)]
pub fn gpu_blit_from_phys(src_phys: usize, len: usize) -> isize {
    if len < DOOM_BLIT_BYTES {
        log::error!("[GPU] Blit: len too small");
        return -1;
    }
    unsafe {
        let src = core::slice::from_raw_parts(src_phys as *const u8, DOOM_BLIT_BYTES);
        gpu_blit_from_slice(src)
    }
}

/// 将 DOOM 调色板索引展开为 RGB 颜色
#[allow(dead_code)]
fn doom_palette_expand(color_idx: usize) -> (u8, u8, u8) {
    match color_idx {
        0   => (0x00, 0x00, 0x00),   1   => (0x1d, 0x1d, 0x1d),
        2   => (0x33, 0x33, 0x33),   3   => (0x4d, 0x4d, 0x4d),
        4   => (0x66, 0x66, 0x66),   5   => (0x80, 0x80, 0x80),
        6   => (0x99, 0x99, 0x99),   7   => (0xb3, 0xb3, 0xb3),
        8   => (0xcc, 0xcc, 0xcc),   9   => (0xe6, 0xe6, 0xe6),
        10  => (0xff, 0xff, 0xff),   11  => (0x1d, 0x00, 0x00),
        12  => (0x00, 0x1d, 0x00),   13  => (0x1d, 0x1d, 0x00),
        14  => (0x00, 0x00, 0x1d),   15  => (0x1d, 0x00, 0x1d),
        16  => (0x00, 0x1d, 0x1d),   17  => (0x41, 0x00, 0x00),
        18  => (0x00, 0x41, 0x00),   19  => (0x41, 0x41, 0x00),
        20  => (0x00, 0x00, 0x41),   21  => (0x41, 0x00, 0x41),
        22  => (0x00, 0x41, 0x41),   23  => (0x41, 0x41, 0x41),
        24  => (0x63, 0x00, 0x00),   25  => (0x00, 0x63, 0x00),
        26  => (0x63, 0x63, 0x00),   27  => (0x00, 0x00, 0x63),
        28  => (0x63, 0x00, 0x63),   29  => (0x00, 0x63, 0x63),
        30  => (0x63, 0x63, 0x63),   31  => (0x00, 0x00, 0x00),
        32  => (0x00, 0x00, 0x00),   33  => (0xff, 0xff, 0xff),
        34  => (0xff, 0x00, 0x00),   35  => (0x00, 0xff, 0x00),
        36  => (0xff, 0xff, 0x00),   37  => (0x00, 0x00, 0xff),
        38  => (0xff, 0x00, 0xff),   39  => (0x00, 0xff, 0xff),
        40  => (0x7f, 0x00, 0x00),   41  => (0x00, 0x7f, 0x00),
        42  => (0x7f, 0x7f, 0x00),   43  => (0x00, 0x00, 0x7f),
        44  => (0x7f, 0x00, 0x7f),   45  => (0x00, 0x7f, 0x7f),
        46  => (0x7f, 0x7f, 0x7f),   47  => (0xbf, 0x00, 0x00),
        48  => (0x00, 0xbf, 0x00),   49  => (0xbf, 0xbf, 0x00),
        50  => (0x00, 0x00, 0xbf),   51  => (0xbf, 0x00, 0xbf),
        52  => (0x00, 0xbf, 0xbf),   53  => (0xbf, 0xbf, 0xbf),
        54  => (0xff, 0x1d, 0x1d),   55  => (0x1d, 0xff, 0x1d),
        56  => (0xff, 0xff, 0x1d),   57  => (0x1d, 0x1d, 0xff),
        58  => (0xff, 0x1d, 0xff),   59  => (0x1d, 0xff, 0xff),
        60  => (0xff, 0xbf, 0x3f),   61  => (0x3f, 0xff, 0xbf),
        62  => (0xbf, 0xff, 0x3f),   63  => (0x3f, 0xbf, 0xff),
        64  => (0xff, 0x7f, 0x3f),   65  => (0x3f, 0xff, 0x7f),
        66  => (0x7f, 0xff, 0x3f),   67  => (0x3f, 0x7f, 0xff),
        68  => (0xff, 0x3f, 0x3f),   69  => (0x3f, 0xff, 0x3f),
        70  => (0xff, 0xff, 0x3f),   71  => (0x3f, 0x3f, 0xff),
        72  => (0xff, 0x3f, 0xff),   73  => (0x3f, 0xff, 0xff),
        74  => (0x1d, 0x1d, 0x1d),   75  => (0x2d, 0x2d, 0x2d),
        76  => (0x3d, 0x3d, 0x3d),   77  => (0x4d, 0x4d, 0x4d),
        78  => (0x5d, 0x5d, 0x5d),   79  => (0x6d, 0x6d, 0x6d),
        80  => (0x7d, 0x7d, 0x7d),   81  => (0x8d, 0x8d, 0x8d),
        82  => (0x9d, 0x9d, 0x9d),   83  => (0xad, 0xad, 0xad),
        84  => (0xbd, 0xbd, 0xbd),   85  => (0xcd, 0xcd, 0xcd),
        86  => (0xdd, 0xdd, 0xdd),   87  => (0xed, 0xed, 0xed),
        88  => (0xfb, 0xfb, 0xfb),   89  => (0x0b, 0x0b, 0x0b),
        90  => (0x1b, 0x1b, 0x1b),   91  => (0x2b, 0x2b, 0x2b),
        92  => (0x3b, 0x3b, 0x3b),   93  => (0x4b, 0x4b, 0x4b),
        94  => (0x5b, 0x5b, 0x5b),   95  => (0x6b, 0x6b, 0x6b),
        96  => (0x7b, 0x7b, 0x7b),   97  => (0x8b, 0x8b, 0x8b),
        98  => (0x9b, 0x9b, 0x9d),   99  => (0xab, 0xab, 0xab),
        100 => (0xbb, 0xbb, 0xbb),   101 => (0xcb, 0xcb, 0xcb),
        102 => (0xdb, 0xdb, 0xdb),   103 => (0xeb, 0xeb, 0xeb),
        104 => (0x0b, 0x0b, 0x0b),   105 => (0x2b, 0x2b, 0x2b),
        106 => (0x4b, 0x4b, 0x4b),   107 => (0x6b, 0x6b, 0x6b),
        108 => (0x8b, 0x8b, 0x8b),   109 => (0xab, 0xab, 0xab),
        110 => (0xcb, 0xcb, 0xcb),   111 => (0xeb, 0xeb, 0xeb),
        112 => (0x03, 0x03, 0x03),   113 => (0x1b, 0x1b, 0x1b),
        114 => (0x33, 0x33, 0x33),   115 => (0x4b, 0x4b, 0x4b),
        116 => (0x63, 0x63, 0x63),   117 => (0x7b, 0x7b, 0x7b),
        118 => (0x93, 0x93, 0x93),   119 => (0xab, 0xab, 0xab),
        120 => (0x03, 0x03, 0x03),   121 => (0x1f, 0x1f, 0x1f),
        122 => (0x3b, 0x3b, 0x3b),   123 => (0x57, 0x57, 0x57),
        124 => (0x73, 0x73, 0x73),   125 => (0x8f, 0x8f, 0x8f),
        126 => (0xab, 0xab, 0xab),   127 => (0xc7, 0xc7, 0xc7),
        128 => (0xff, 0x00, 0x00),   129 => (0xff, 0x40, 0x40),
        130 => (0xff, 0x80, 0x80),   131 => (0xff, 0xbf, 0xbf),
        132 => (0xbf, 0x00, 0x00),   133 => (0xbf, 0x3f, 0x3f),
        134 => (0xbf, 0x7f, 0x7f),   135 => (0xbf, 0xbf, 0xbf),
        136 => (0x80, 0x00, 0x00),   137 => (0x80, 0x3f, 0x3f),
        138 => (0x80, 0x7f, 0x7f),   139 => (0x80, 0xbf, 0xbf),
        140 => (0x40, 0x00, 0x00),   141 => (0x40, 0x3f, 0x3f),
        142 => (0x40, 0x7f, 0x7f),   143 => (0x40, 0xbf, 0xbf),
        144 => (0xff, 0xff, 0x00),   145 => (0xff, 0xff, 0x40),
        146 => (0xff, 0xff, 0x80),   147 => (0xff, 0xff, 0xbf),
        148 => (0xbf, 0xbf, 0x00),   149 => (0xbf, 0xbf, 0x3f),
        150 => (0xbf, 0xbf, 0x7f),   151 => (0xbf, 0xbf, 0xbf),
        152 => (0x80, 0x80, 0x00),   153 => (0x80, 0x80, 0x3f),
        154 => (0x80, 0x80, 0x7f),   155 => (0x80, 0x80, 0xbf),
        156 => (0x40, 0x40, 0x00),   157 => (0x40, 0x40, 0x3f),
        158 => (0x40, 0x40, 0x7f),   159 => (0x40, 0x40, 0xbf),
        160 => (0x00, 0xff, 0x00),   161 => (0x40, 0xff, 0x40),
        162 => (0x80, 0xff, 0x80),   163 => (0xbf, 0xff, 0xbf),
        164 => (0x00, 0xbf, 0x00),   165 => (0x3f, 0xbf, 0x3f),
        166 => (0x7f, 0xbf, 0x7f),   167 => (0xbf, 0xbf, 0xbf),
        168 => (0x00, 0x80, 0x00),   169 => (0x3f, 0x80, 0x3f),
        170 => (0x7f, 0x80, 0x7f),   171 => (0xbf, 0x80, 0xbf),
        172 => (0x00, 0x40, 0x00),   173 => (0x3f, 0x40, 0x3f),
        174 => (0x7f, 0x40, 0x7f),   175 => (0xbf, 0x40, 0xbf),
        176 => (0x00, 0xff, 0xff),   177 => (0x40, 0xff, 0xff),
        178 => (0x80, 0xff, 0xff),   179 => (0xbf, 0xff, 0xff),
        180 => (0x00, 0xbf, 0xbf),   181 => (0x3f, 0xbf, 0xbf),
        182 => (0x7f, 0xbf, 0xbf),   183 => (0xbf, 0xbf, 0xbf),
        184 => (0x00, 0x80, 0x80),   185 => (0x3f, 0x80, 0x80),
        186 => (0x7f, 0x80, 0x80),   187 => (0xbf, 0x80, 0x80),
        188 => (0x00, 0x40, 0x40),   189 => (0x3f, 0x40, 0x40),
        190 => (0x7f, 0x40, 0x40),   191 => (0xbf, 0x40, 0x40),
        192 => (0x00, 0x00, 0xff),   193 => (0x40, 0x40, 0xff),
        194 => (0x80, 0x80, 0xff),   195 => (0xbf, 0xbf, 0xff),
        196 => (0x00, 0x00, 0xbf),   197 => (0x3f, 0x3f, 0xbf),
        198 => (0x7f, 0x7f, 0xbf),   199 => (0xbf, 0xbf, 0xbf),
        200 => (0x00, 0x00, 0x80),   201 => (0x3f, 0x3f, 0x80),
        202 => (0x7f, 0x7f, 0x80),   203 => (0xbf, 0xbf, 0x80),
        204 => (0x00, 0x00, 0x40),   205 => (0x3f, 0x3f, 0x40),
        206 => (0x7f, 0x7f, 0x40),   207 => (0xbf, 0xbf, 0x40),
        208 => (0xff, 0x00, 0xff),   209 => (0xff, 0x40, 0xff),
        210 => (0xff, 0x80, 0xff),   211 => (0xff, 0xbf, 0xff),
        212 => (0xbf, 0x00, 0xbf),   213 => (0xbf, 0x3f, 0xbf),
        214 => (0xbf, 0x7f, 0xbf),   215 => (0xbf, 0xbf, 0xbf),
        216 => (0x80, 0x00, 0x80),   217 => (0x80, 0x3f, 0x80),
        218 => (0x80, 0x7f, 0x80),   219 => (0x80, 0xbf, 0x80),
        220 => (0x40, 0x00, 0x40),   221 => (0x40, 0x3f, 0x40),
        222 => (0x40, 0x7f, 0x40),   223 => (0x40, 0xbf, 0x40),
        224 => (0xff, 0x80, 0x00),   225 => (0xff, 0x9f, 0x1f),
        226 => (0xff, 0xbf, 0x3f),   227 => (0xff, 0xdf, 0x5f),
        228 => (0xff, 0xff, 0x7f),   229 => (0xff, 0xdf, 0x9f),
        230 => (0xff, 0xbf, 0xbf),   231 => (0xff, 0x9f, 0xdf),
        232 => (0xdf, 0x80, 0x00),   233 => (0xdf, 0x9f, 0x1f),
        234 => (0xdf, 0xbf, 0x3f),   235 => (0xdf, 0xdf, 0x5f),
        236 => (0xdf, 0xff, 0x7f),   237 => (0xdf, 0xdf, 0x9f),
        238 => (0xdf, 0xbf, 0xbf),   239 => (0xdf, 0x9f, 0xdf),
        240 => (0xbf, 0x80, 0x00),   241 => (0xbf, 0x9f, 0x1f),
        242 => (0xbf, 0xbf, 0x3f),   243 => (0xbf, 0xdf, 0x5f),
        244 => (0xbf, 0xff, 0x7f),   245 => (0xbf, 0xdf, 0x9f),
        246 => (0xbf, 0xbf, 0xbf),   247 => (0xbf, 0x9f, 0xdf),
        248 => (0x9f, 0x80, 0x00),   249 => (0x9f, 0x9f, 0x1f),
        250 => (0x9f, 0xbf, 0x3f),   251 => (0x9f, 0xdf, 0x5f),
        252 => (0x9f, 0xff, 0x7f),   253 => (0x9f, 0xdf, 0x9f),
        254 => (0x9f, 0xbf, 0xbf),   255 => (0x9f, 0x9f, 0xdf),
        _ => (color_idx as u8, color_idx as u8, color_idx as u8),
    }
}
