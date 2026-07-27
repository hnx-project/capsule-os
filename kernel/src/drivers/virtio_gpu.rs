//! # 🎨 Virtio-GPU MMIO Legacy Driver
//!
//! Handles initialization, memory mapping, and frame buffer flushing for the Virtio-GPU
//! MMIO graphics device on AArch64 QEMU Virt.

use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::arch::ArchHardware;
use crate::arch::aarch64::phys::{alloc_page, PageTag, PhysAddr};
use crate::arch::mmu_facade::pa_to_kernel_va;

static VIRTIO_GPU_BASE: AtomicUsize = AtomicUsize::new(0);

const QUEUE_SIZE: usize = 16;
const SCREEN_WIDTH: u32 = 400;
const SCREEN_HEIGHT: u32 = 300;

// Virtio Descriptor Table entry
#[repr(C, align(16))]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

// Virtio-GPU Control Header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuCtrlHdr {
    pub r#type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuRespHdr {
    pub r#type: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuResourceCreate2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32, // B8G8R8A8_UNORM = 3
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuResourceAttachBackingCombined {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
    pub entry: VirtioGpuMemEntry, // Directly packed inline with zero padding
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuSetScanout {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuTransferToHost2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuResourceFlush {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub resource_id: u32,
    pub padding: u32,
}

// Globals for Virtqueues
static mut DESC_TABLE: *mut VirtqDesc = core::ptr::null_mut();
static mut AVAIL_RING: *mut u16 = core::ptr::null_mut();
static mut USED_RING: *mut u16 = core::ptr::null_mut();
static mut AVAIL_IDX: u16 = 0;

// Dedicated, contiguous DMA-safe command and response buffers
static mut GPU_REQ_CREATE: *mut VirtioGpuResourceCreate2d = core::ptr::null_mut();
static mut GPU_REQ_ATTACH_COMBINED: *mut VirtioGpuResourceAttachBackingCombined = core::ptr::null_mut();
static mut GPU_REQ_SCANOUT: *mut VirtioGpuSetScanout = core::ptr::null_mut();
static mut GPU_REQ_TRANSFER: *mut VirtioGpuTransferToHost2d = core::ptr::null_mut();
static mut GPU_REQ_FLUSH: *mut VirtioGpuResourceFlush = core::ptr::null_mut();
static mut GPU_RESP: *mut VirtioGpuRespHdr = core::ptr::null_mut();

// Physical address of our contiguous GPU Framebuffer backplane
static mut GPU_BACKING_PHYS_ADDR: usize = 0;

unsafe fn flush_cache(va: usize, len: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        <crate::arch::CurrentArch as ArchHardware>::clean_and_invalidate_cache_range(va, len);
    }
}

/// Initialize and probe Virtio-MMIO GPU devices starting from 0x0a000000.
pub fn init() {
    for i in 0..32 {
        let base = 0x0a000000 + i * 0x200;
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        let dev_id = unsafe { core::ptr::read_volatile((base + 0x008) as *const u32) };
        if magic == 0x74726976 && dev_id == 16 { // 16 = GPU Device
            crate::log_info!("VIRTIO", "Discovered Virtio-GPU device at slot {} MMIO {:#x}", i, base);
            match unsafe { init_gpu_device(base) } {
                Ok(()) => {
                    VIRTIO_GPU_BASE.store(base, Ordering::SeqCst);
                    crate::log_info!("VIRTIO", "Virtio-GPU device at {:#x} successfully initialized!", base);
                    break;
                }
                Err(e) => {
                    crate::log_error!("VIRTIO", "Failed to initialize Virtio-GPU at {:#x}: {:?}", base, e);
                }
            }
        }
    }
}

unsafe fn init_gpu_device(base: usize) -> Result<()> {
    // 1. Reset device
    core::ptr::write_volatile(base as *mut u32, 0);

    // 2. Set status to ACKNOWLEDGE and DRIVER
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2);

    // 3. Negotiate features
    let f0 = core::ptr::read_volatile((base + 0x010) as *const u32);
    core::ptr::write_volatile((base + 0x020) as *mut u32, f0); // Accept all offered features

    // Set FEATURES_OK status bit
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8);

    // Set GuestPageSize to 4096
    core::ptr::write_volatile((base + 0x028) as *mut u32, 4096);

    // 4. Select and configure Virtqueue 0 (Control Queue)
    core::ptr::write_volatile((base + 0x030) as *mut u32, 0); // QueueSel = 0
    let max_size = core::ptr::read_volatile((base + 0x034) as *const u32);
    if max_size < QUEUE_SIZE as u32 {
        return Err(Status::NotAllowed);
    }
    core::ptr::write_volatile((base + 0x038) as *mut u32, QUEUE_SIZE as u32); // QueueNum
    core::ptr::write_volatile((base + 0x03c) as *mut u32, 4096); // QueueAlign

    // 5. Allocate contiguous physical pages for the Virtqueue rings
    let page0 = alloc_page(PageTag::KernelHeap)?;
    let page1 = alloc_page(PageTag::KernelHeap)?;
    if page1.as_usize() != page0.as_usize() + 4096 {
        panic!("VIRTIO_GPU: Allocated pages are not contiguous!");
    }

    let kva0 = pa_to_kernel_va(page0.as_usize());
    core::ptr::write_bytes(kva0 as *mut u8, 0, 8192);

    DESC_TABLE = kva0 as *mut VirtqDesc;
    AVAIL_RING = (kva0 + 256) as *mut u16;
    USED_RING = (kva0 + 4096) as *mut u16;

    // Tell device of queue location
    core::ptr::write_volatile((base + 0x040) as *mut u32, (page0.as_usize() >> 12) as u32);

    // Allocate command request & response structures
    let buf_page = alloc_page(PageTag::KernelHeap)?;
    let buf_kva = pa_to_kernel_va(buf_page.as_usize());
    core::ptr::write_bytes(buf_kva as *mut u8, 0, 4096);

    GPU_REQ_CREATE = buf_kva as *mut VirtioGpuResourceCreate2d;
    GPU_REQ_ATTACH_COMBINED = (buf_kva + 256) as *mut VirtioGpuResourceAttachBackingCombined;
    GPU_REQ_SCANOUT = (buf_kva + 768) as *mut VirtioGpuSetScanout;
    GPU_REQ_TRANSFER = (buf_kva + 1024) as *mut VirtioGpuTransferToHost2d;
    GPU_REQ_FLUSH = (buf_kva + 1280) as *mut VirtioGpuResourceFlush;
    GPU_RESP = (buf_kva + 2048) as *mut VirtioGpuRespHdr;

    // Allocate 120 contiguous pages for our 400x300 graphical framebuffer (roughly 480KB)
    let mut backing_start_pa = 0;
    for i in 0..120 {
        let page = alloc_page(PageTag::KernelHeap)?;
        if i == 0 {
            backing_start_pa = page.as_usize();
        } else if page.as_usize() != backing_start_pa + i * 4096 {
            panic!("VIRTIO_GPU: Framebuffer pages are not contiguous!");
        }
    }
    // Zero-initialize the framebuffer backplane
    core::ptr::write_bytes(pa_to_kernel_va(backing_start_pa) as *mut u8, 0, 120 * 4096);
    GPU_BACKING_PHYS_ADDR = backing_start_pa;

    // Set DRIVER_OK status
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8 | 4);

    // 6. Build and submit standard GPU resource pipeline setup
    let resource_id = 1u32;

    // A. Create 2D Host Resource
    core::ptr::write_volatile(GPU_REQ_CREATE, VirtioGpuResourceCreate2d {
        hdr: VirtioGpuCtrlHdr {
            r#type: 0x0101, // VIRTIO_GPU_CMD_RESOURCE_CREATE_2D
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        resource_id,
        format: 3, // B8G8R8A8_UNORM
        width: SCREEN_WIDTH,
        height: SCREEN_HEIGHT,
    });
    submit_command(base, GPU_REQ_CREATE as usize, core::mem::size_of::<VirtioGpuResourceCreate2d>())?;

    // B. Attach Backing Physical Storage (zero-copy DMA mapping, combining header and layout entries with zero padding)
    core::ptr::write_volatile(GPU_REQ_ATTACH_COMBINED, VirtioGpuResourceAttachBackingCombined {
        hdr: VirtioGpuCtrlHdr {
            r#type: 0x0106, // VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        resource_id,
        nr_entries: 1,
        entry: VirtioGpuMemEntry {
            addr: GPU_BACKING_PHYS_ADDR as u64,
            length: 120 * 4096,
            padding: 0,
        },
    });
    submit_command(base, GPU_REQ_ATTACH_COMBINED as usize, core::mem::size_of::<VirtioGpuResourceAttachBackingCombined>())?;

    // C. Bind to Screen Display Scanout
    core::ptr::write_volatile(GPU_REQ_SCANOUT, VirtioGpuSetScanout {
        hdr: VirtioGpuCtrlHdr {
            r#type: 0x0103, // VIRTIO_GPU_CMD_SET_SCANOUT
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            padding: 0,
        },
        r: VirtioGpuRect {
            x: 0,
            y: 0,
            width: SCREEN_WIDTH,
            height: SCREEN_HEIGHT,
        },
        scanout_id: 0,
        resource_id,
    });
    submit_command(base, GPU_REQ_SCANOUT as usize, core::mem::size_of::<VirtioGpuSetScanout>())?;

    Ok(())
}

unsafe fn submit_command(base: usize, cmd_kva: usize, cmd_len: usize) -> Result<()> {
    let cmd_type = core::ptr::read_volatile(cmd_kva as *const u32);
    crate::log_info!("VIRTIO_GPU", "Submitting command 0x{:x}...", cmd_type);

    // Clear response buffer
    core::ptr::write_bytes(GPU_RESP as *mut u8, 0xff, core::mem::size_of::<VirtioGpuRespHdr>());
    flush_cache(GPU_RESP as usize, core::mem::size_of::<VirtioGpuRespHdr>());

    // Desc 0: Command (Read-Only)
    let cmd_pa = cmd_kva - 0xffff800000000000;
    *DESC_TABLE.add(0) = VirtqDesc {
        addr: cmd_pa as u64,
        len: cmd_len as u32,
        flags: VIRTQ_DESC_F_NEXT,
        next: 1,
    };

    // Desc 1: Response (Write-Only)
    let resp_pa = GPU_RESP as usize - 0xffff800000000000;
    *DESC_TABLE.add(1) = VirtqDesc {
        addr: resp_pa as u64,
        len: core::mem::size_of::<VirtioGpuRespHdr>() as u32,
        flags: VIRTQ_DESC_F_WRITE,
        next: 0,
    };

    // Put Desc 0 on available ring
    let ring_idx_offset = (AVAIL_IDX % QUEUE_SIZE as u16) as usize;
    core::ptr::write_volatile(AVAIL_RING.add(2 + ring_idx_offset), 0);
    AVAIL_IDX = AVAIL_IDX.wrapping_add(1);
    core::ptr::write_volatile(AVAIL_RING.add(1), AVAIL_IDX);

    // Flush modified CPU caches
    flush_cache(cmd_kva, cmd_len);
    flush_cache(DESC_TABLE as usize, QUEUE_SIZE * 16);
    flush_cache(AVAIL_RING as usize, 256);

    #[cfg(target_arch = "aarch64")]
    core::arch::asm!("dsb sy");

    // Notify card
    core::ptr::write_volatile((base + 0x050) as *mut u32, 0); // QueueNotify = 0

    // Poll wait for Used Ring update or response type
    loop {
        flush_cache(GPU_RESP as usize, core::mem::size_of::<VirtioGpuRespHdr>());
        if core::ptr::read_volatile(&(*GPU_RESP).r#type) != 0xffffffff {
            break;
        }
        core::hint::spin_loop();
    }

    let resp_type = core::ptr::read_volatile(&(*GPU_RESP).r#type);
    if resp_type == 0x1100 { // VIRTIO_GPU_RESP_OK_NODATA
        Ok(())
    } else {
        crate::log_error!("VIRTIO_GPU", "Command failed! Response type: 0x{:x}", resp_type);
        Err(Status::InvalidArgs)
    }
}

/// Copies a single physical page frame of pixel data into our local hardware framebuffer backplane.
pub fn copy_to_gpu_buffer(src_pa: usize, offset: usize) {
    if offset + 4096 > 120 * 4096 {
        return;
    }
    unsafe {
        let dest_kva = pa_to_kernel_va(GPU_BACKING_PHYS_ADDR) + offset;
        let src_kva = pa_to_kernel_va(src_pa);
        // Copy 4KB page contiguously
        core::ptr::copy_nonoverlapping(src_kva as *const u8, dest_kva as *mut u8, 4096);
        // Flush dest to RAM so GPU DMA can read it
        flush_cache(dest_kva, 4096);
    }
}

/// Commit and flush our local GPU backplane buffer to the host screen display window.
pub fn flush_to_screen() {
    let base = VIRTIO_GPU_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }
    let resource_id = 1u32;

    unsafe {
        // 1. Transfer RAM Backplane content to host 2D resource
        core::ptr::write_volatile(GPU_REQ_TRANSFER, VirtioGpuTransferToHost2d {
            hdr: VirtioGpuCtrlHdr {
                r#type: 0x0105, // VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            r: VirtioGpuRect {
                x: 0,
                y: 0,
                width: SCREEN_WIDTH,
                height: SCREEN_HEIGHT,
            },
            offset: 0,
            resource_id,
            padding: 0,
        });
        let _ = submit_command(base, GPU_REQ_TRANSFER as usize, core::mem::size_of::<VirtioGpuTransferToHost2d>());

        // 2. Instruct QEMU SDL window to flush and repaint the Scanout display
        core::ptr::write_volatile(GPU_REQ_FLUSH, VirtioGpuResourceFlush {
            hdr: VirtioGpuCtrlHdr {
                r#type: 0x0104, // VIRTIO_GPU_CMD_RESOURCE_FLUSH
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            r: VirtioGpuRect {
                x: 0,
                y: 0,
                width: SCREEN_WIDTH,
                height: SCREEN_HEIGHT,
            },
            resource_id,
            padding: 0,
        });
        let _ = submit_command(base, GPU_REQ_FLUSH as usize, core::mem::size_of::<VirtioGpuResourceFlush>());
    }
}
