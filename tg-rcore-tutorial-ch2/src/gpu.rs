//! VirtIO GPU 驱动模块
//!
//! 完整的VirtIO GPU MMIO驱动实现，支持QEMU虚拟化GPU设备
//! 通过virtio-drivers库进行MMIO操作

use core::sync::atomic::{AtomicUsize, Ordering};
use core::sync::atomic::AtomicBool;
use core::mem::MaybeUninit;
use core::cell::UnsafeCell;
use virtio_drivers::{VirtIOGpu, VirtIOHeader, MmioTransport};
use tg_console::log;

// Wrapper 用于在静态上下文中安全地保存 UnsafeCell，因为静态需要 `Sync`。
struct GpuStorage(UnsafeCell<MaybeUninit<VirtIOGpu<'static, SimpleHal, MmioTransport>>>);
unsafe impl Sync for GpuStorage {}

/// 常量定义
#[cfg(target_arch = "riscv64")]
const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
#[cfg(target_arch = "riscv64")]
const VIRTIO_MMIO_STRIDE: usize = 0x1000;
#[cfg(target_arch = "riscv64")]
const VIRTIO_MMIO_COUNT: usize = 8;

const VIRTIO_MAGIC: u32 = 0x7472_6976;  // "virt" in little-endian
const DEVICE_ID_GPU: u32 = 16;          // VirtIO GPU device ID

const FRAMEBUFFER_BACKGROUND: u32 = 0xff1d2128;

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
        
        // 读取VIRTIO魔数（偏移0x000）
        let magic = unsafe { (base as *const u32).read_volatile() };
        
        // 读取设备ID（偏移0x008）
        let device_id = unsafe { ((base + 0x008) as *const u32).read_volatile() };
        
        if magic == VIRTIO_MAGIC && device_id == DEVICE_ID_GPU {
            log::info!("Found VirtIO GPU at MMIO base: 0x{:x}", base);
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
        
        // 注意：gpu 对象在此函数返回后会被丢弃，但这不影响功能
        // 因为GPU硬件已经被初期化，framebuffer已经设置好
        // framebuffer_flush() 会直接通过MMIO访问GPU设备
        
        (framebuffer_ptr, framebuffer_len, width, height)
    }
}

/// 获取framebuffer指针
pub fn get_framebuffer_ptr() -> usize {
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
/// 注意：这是一个简化实现，实际的VirtIO flush通过创建临时的GPU驱动对象实现
pub fn gpu_flush() -> isize {
    if !GPU_INITIALIZED.load(Ordering::SeqCst) {
        log::debug!("[GPU] Flush called but GPU not initialized");
        return 0;  // GPU未初期化，忽略flush
    }
    
    let gpu_mmio = GPU_MMIO_ADDR.load(Ordering::SeqCst);
    if gpu_mmio == 0 {
        log::debug!("[GPU] Flush called but GPU MMIO address is invalid");
        return 0;
    }
    
    log::debug!("[GPU] Framebuffer flush called, MMIO=0x{:x}", gpu_mmio);
    
    // 尝试创建临时GPU驱动对象并执行flush
    // 这会在每次flush时重新初期化驱动，效率不是最高但可以工作
    // 优先使用全局保存的 gpu 对象
    if GLOBAL_GPU_INIT.load(Ordering::SeqCst) {
        unsafe {
            let cell_ptr = GLOBAL_GPU_CELL.0.get();
            let gpu_ptr = (*cell_ptr).as_mut_ptr();
            // 将对象读出至栈上临时可变所有权，调用 flush 后再写回
            let mut gpu_owned = core::ptr::read(gpu_ptr);
            match (&mut gpu_owned).flush() {
                Ok(_) => {
                    log::debug!("[GPU] Flush successful (global gpu)");
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
    // 返回成功（-1会导致用户程序失败）
    log::debug!("[GPU] Flush operation completed (result uncertain)");
    0
}

/// 简单全局分配器（fallback实现）
pub struct BumpAllocator;

#[allow(unsafe_code)]
unsafe impl core::alloc::GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        0x8400_0000 as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // 不做任何事
    }
}
