//! # 💾 Virtio-Block MMIO Legacy Driver
//!
//! Handles initialization, sector-level reading, and sector-level writing for Virtio-Block
//! MMIO devices on the AArch64 QEMU Virt platform.

use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::arch::ArchHardware;
use crate::arch::aarch64::phys::{alloc_page, PageTag, PhysAddr};
use crate::arch::mmu_facade::pa_to_kernel_va;

/// The selected Virtio MMIO block device base address. 0 means not found/uninitialized.
static VIRTIO_BLK_BASE: AtomicUsize = AtomicUsize::new(0);

/// Helper to flush and invalidate CPU caches on AArch64.
unsafe fn flush_cache(va: usize, len: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        <crate::arch::CurrentArch as ArchHardware>::clean_and_invalidate_cache_range(va, len);
    }
}

/// Virtqueue sizes (must be a power of 2).
const QUEUE_SIZE: usize = 16;

/// Virtio Descriptor Table entry.
#[repr(C, align(16))]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

/// Virtio Block Request Header.
#[repr(C)]
#[derive(Default)]
struct VirtIOBlkHeader {
    r#type: u32,
    reserved: u32,
    sector: u64,
}

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

/// Globals pointing to our physically contiguous 2-page Virtqueue block.
static mut DESC_TABLE: *mut VirtqDesc = core::ptr::null_mut();
static mut AVAIL_RING: *mut u16 = core::ptr::null_mut();
static mut USED_RING: *mut u16 = core::ptr::null_mut();

/// Dedicated, contiguous buffers to avoid DMA race conditions and alignment issues.
static mut BLK_HEADER: *mut VirtIOBlkHeader = core::ptr::null_mut();
static mut BLK_STATUS: *mut u8 = core::ptr::null_mut();

/// Current position in the Available Ring.
static mut AVAIL_IDX: u16 = 0;
/// Expected position in the Used Ring.
static mut USED_IDX: u16 = 0;

/// Initialize and probe Virtio-MMIO block devices starting from 0x0a000000.
pub fn init() {
    for i in 0..32 {
        let base = 0x0a000000 + i * 0x200;
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        let version = unsafe { core::ptr::read_volatile((base + 0x004) as *const u32) };
        let dev_id = unsafe { core::ptr::read_volatile((base + 0x008) as *const u32) };
        if magic == 0x74726976 { // "virt"
            if dev_id == 2 { // Block Device
                crate::log_info!("VIRTIO", "Discovered Virtio-Blk device at slot {} MMIO {:#x} Version {}", i, base, version);
                match unsafe { init_device(base) } {
                    Ok(()) => {
                        VIRTIO_BLK_BASE.store(base, Ordering::SeqCst);
                        crate::log_info!("VIRTIO", "Virtio-Blk device at {:#x} successfully initialized!", base);
                        break;
                    }
                    Err(e) => {
                        crate::log_error!("VIRTIO", "Failed to initialize Virtio-Blk device at {:#x}: {:?}", base, e);
                    }
                }
            }
        }
    }
}

/// Legacy Virtio MMIO Block Device Initialization.
unsafe fn init_device(base: usize) -> Result<()> {
    // 1. Reset device
    core::ptr::write_volatile((base + 0x070) as *mut u32, 0); // Status = 0
    
    // 2. Acknowledge & Driver status bits
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1); // Status = ACKNOWLEDGE
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2); // Status = ACKNOWLEDGE | DRIVER

    // 3. Negotiate features
    core::ptr::write_volatile((base + 0x014) as *mut u32, 0); // DeviceFeaturesSel = 0
    let f0 = core::ptr::read_volatile((base + 0x010) as *const u32);
    core::ptr::write_volatile((base + 0x014) as *mut u32, 1); // DeviceFeaturesSel = 1
    let f1 = core::ptr::read_volatile((base + 0x010) as *const u32);
    crate::log_info!("VIRTIO", "Offered features: page0={:#x}, page1={:#x}", f0, f1);

    // Minimal block features: SizeMax (bit 2), Geometry (bit 9), BlockSize (bit 11)
    let negotiated = f0 & ((1 << 2) | (1 << 9) | (1 << 11));
    crate::log_info!("VIRTIO", "Negotiated features: {:#x}", negotiated);

    core::ptr::write_volatile((base + 0x024) as *mut u32, 0); // DriverFeaturesSel = 0
    core::ptr::write_volatile((base + 0x020) as *mut u32, negotiated);
    core::ptr::write_volatile((base + 0x024) as *mut u32, 1); // DriverFeaturesSel = 1
    core::ptr::write_volatile((base + 0x020) as *mut u32, 0); // We don't support any page 1 features

    // Set FEATURES_OK status bit
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8); // ACKNOWLEDGE | DRIVER | FEATURES_OK
    let features_ok_status = core::ptr::read_volatile((base + 0x070) as *const u32);
    crate::log_info!("VIRTIO", "Status after FEATURES_OK: {:#x}", features_ok_status);

    // Set GuestPageSize (0x028) to 4096 so the device knows our page size for PFN calculation
    core::ptr::write_volatile((base + 0x028) as *mut u32, 4096);

    // 4. Select and configure Virtqueue 0
    core::ptr::write_volatile((base + 0x030) as *mut u32, 0); // QueueSel = 0
    let max_size = core::ptr::read_volatile((base + 0x034) as *const u32);
    crate::log_info!("VIRTIO", "Queue 0 maximum size: {}", max_size);
    if max_size < QUEUE_SIZE as u32 {
        return Err(Status::NotAllowed);
    }
    core::ptr::write_volatile((base + 0x038) as *mut u32, QUEUE_SIZE as u32); // QueueNum = QUEUE_SIZE
    core::ptr::write_volatile((base + 0x03c) as *mut u32, 4096); // QueueAlign = 4096

    // 5. Allocate 2 contiguous physical pages (using the cursor guarantee)
    let page0 = alloc_page(PageTag::KernelHeap)?;
    let page1 = alloc_page(PageTag::KernelHeap)?;
    if page1.as_usize() != page0.as_usize() + 4096 {
        panic!("VIRTIO: Allocated pages are not contiguous! page0={:#x}, page1={:#x}", page0.as_usize(), page1.as_usize());
    }

    // Zero initialize pages
    let kva0 = pa_to_kernel_va(page0.as_usize());
    core::ptr::write_bytes(kva0 as *mut u8, 0, 8192);

    // Layout:
    // Page 0 (offset 0): Descriptor Table (16 * 16 = 256 bytes)
    // Page 0 (offset 256): Available Ring
    //   - flags: u16 (2 bytes)
    //   - idx: u16 (2 bytes)
    //   - ring: [u16; 16] (32 bytes)
    // Page 1 (offset 4096): Used Ring (aligned to 4096)
    DESC_TABLE = kva0 as *mut VirtqDesc;
    AVAIL_RING = (kva0 + 256) as *mut u16;
    USED_RING = (kva0 + 4096) as *mut u16;

    // Allocate dedicated buffers for request headers and status byte
    let buf_page = alloc_page(PageTag::KernelHeap)?;
    let buf_kva = pa_to_kernel_va(buf_page.as_usize());
    core::ptr::write_bytes(buf_kva as *mut u8, 0, 4096);
    BLK_HEADER = buf_kva as *mut VirtIOBlkHeader;
    BLK_STATUS = (buf_kva + 1024) as *mut u8;

    // Tell the device where the queue is located in physical memory (PFN)
    core::ptr::write_volatile((base + 0x040) as *mut u32, (page0.as_usize() >> 12) as u32); // QueuePFN

    // 6. Set DRIVER_OK status
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8 | 4); // Status = ACKNOWLEDGE | DRIVER | FEATURES_OK | DRIVER_OK
    let final_status = core::ptr::read_volatile((base + 0x070) as *const u32);
    crate::log_info!("VIRTIO", "Final device status register value: {:#x}", final_status);

    // Read and log disk capacity
    let cap_low = core::ptr::read_volatile((base + 0x100) as *const u32) as u64;
    let cap_high = core::ptr::read_volatile((base + 0x104) as *const u32) as u64;
    let capacity = cap_low | (cap_high << 32);
    crate::log_info!("VIRTIO", "Discovered disk capacity: {} sectors ({} KB)", capacity, capacity * 512 / 1024);

    Ok(())
}

/// Read a single 512-byte sector from the disk into the physical address specified.
pub fn read_sector(sector: u64, dst_pa: usize) -> Result<()> {
    let base = VIRTIO_BLK_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return Err(Status::NotAllowed); // Driver not loaded
    }

    unsafe {
        // Construct the request header
        core::ptr::write_volatile(BLK_HEADER, VirtIOBlkHeader {
            r#type: VIRTIO_BLK_T_IN,
            reserved: 0,
            sector,
        });
        core::ptr::write_volatile(BLK_STATUS, 0xFF); // Initialize status to non-zero

        // Populate three descriptors:
        // Desc 0: Header (read-only by device)
        let header_final_pa = BLK_HEADER as usize - 0xffff800000000000;

        // Desc 2: Status (writeable by device)
        let status_final_pa = BLK_STATUS as usize - 0xffff800000000000;

        *DESC_TABLE.add(0) = VirtqDesc {
            addr: header_final_pa as u64,
            len: core::mem::size_of::<VirtIOBlkHeader>() as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        };

        // Desc 1: Buffer (writeable by device)
        *DESC_TABLE.add(1) = VirtqDesc {
            addr: dst_pa as u64,
            len: 512,
            flags: VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE,
            next: 2,
        };

        // Desc 2: Status (writeable by device)
        *DESC_TABLE.add(2) = VirtqDesc {
            addr: status_final_pa as u64,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };

        // Put descriptor 0 index into the Available Ring
        let ring_idx_offset = (AVAIL_IDX % QUEUE_SIZE as u16) as usize;
        core::ptr::write_volatile(AVAIL_RING.add(2 + ring_idx_offset), 0); // Put Desc 0 in ring
        AVAIL_IDX = AVAIL_IDX.wrapping_add(1);
        core::ptr::write_volatile(AVAIL_RING.add(1), AVAIL_IDX); // Update idx field

        // Clean cache of descriptors, rings, headers, and status byte before notifying device
        flush_cache(BLK_HEADER as usize, core::mem::size_of::<VirtIOBlkHeader>());
        flush_cache(BLK_STATUS as usize, 1);
        flush_cache(DESC_TABLE as usize, QUEUE_SIZE * 16);
        flush_cache(AVAIL_RING as usize, 256);

        // Enforce memory synchronization before notifying the device
        #[cfg(target_arch = "aarch64")]
        core::arch::asm!("dsb sy");

        // Notify the device of new request in queue 0
        core::ptr::write_volatile((base + 0x050) as *mut u32, 0); // QueueNotify = 0

        // Wait (poll) for completion
        loop {
            flush_cache(BLK_STATUS as usize, 1);
            if core::ptr::read_volatile(BLK_STATUS) != 0xFF {
                break;
            }
            core::hint::spin_loop();
        }

        // Invalidate the cache of the destination buffer so CPU reads the fresh data written by DMA
        flush_cache(pa_to_kernel_va(dst_pa), 512);

        if core::ptr::read_volatile(BLK_STATUS) == 0 {
            Ok(())
        } else {
            Err(Status::InvalidArgs)
        }
    }
}

/// Write a single 512-byte sector from the physical address specified to the disk.
pub fn write_sector(sector: u64, src_pa: usize) -> Result<()> {
    let base = VIRTIO_BLK_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return Err(Status::NotAllowed);
    }

    unsafe {
        // Construct the request header
        core::ptr::write_volatile(BLK_HEADER, VirtIOBlkHeader {
            r#type: VIRTIO_BLK_T_OUT,
            reserved: 0,
            sector,
        });
        core::ptr::write_volatile(BLK_STATUS, 0xFF);

        // Populate three descriptors:
        // Desc 0: Header (read-only by device)
        let header_final_pa = BLK_HEADER as usize - 0xffff800000000000;

        *DESC_TABLE.add(0) = VirtqDesc {
            addr: header_final_pa as u64,
            len: core::mem::size_of::<VirtIOBlkHeader>() as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        };

        // Desc 1: Buffer (read-only by device)
        *DESC_TABLE.add(1) = VirtqDesc {
            addr: src_pa as u64,
            len: 512,
            flags: VIRTQ_DESC_F_NEXT,
            next: 2,
        };

        // Desc 2: Status (writeable by device)
        let status_final_pa = BLK_STATUS as usize - 0xffff800000000000;

        *DESC_TABLE.add(2) = VirtqDesc {
            addr: status_final_pa as u64,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };

        // Put descriptor 0 index into the Available Ring
        let ring_idx_offset = (AVAIL_IDX % QUEUE_SIZE as u16) as usize;
        core::ptr::write_volatile(AVAIL_RING.add(2 + ring_idx_offset), 0);
        AVAIL_IDX = AVAIL_IDX.wrapping_add(1);
        core::ptr::write_volatile(AVAIL_RING.add(1), AVAIL_IDX);

        // Clean cache of descriptors, rings, headers, status, and source buffer before notifying device
        flush_cache(BLK_HEADER as usize, core::mem::size_of::<VirtIOBlkHeader>());
        flush_cache(BLK_STATUS as usize, 1);
        flush_cache(DESC_TABLE as usize, QUEUE_SIZE * 16);
        flush_cache(AVAIL_RING as usize, 256);
        flush_cache(pa_to_kernel_va(src_pa), 512);

        // Enforce memory synchronization before notifying the device
        #[cfg(target_arch = "aarch64")]
        core::arch::asm!("dsb sy");

        // Notify the device of new request in queue 0
        core::ptr::write_volatile((base + 0x050) as *mut u32, 0); // QueueNotify = 0

        // Wait (poll) for completion
        loop {
            flush_cache(BLK_STATUS as usize, 1);
            if core::ptr::read_volatile(BLK_STATUS) != 0xFF {
                break;
            }
            core::hint::spin_loop();
        }

        if core::ptr::read_volatile(BLK_STATUS) == 0 {
            Ok(())
        } else {
            Err(Status::InvalidArgs)
        }
    }
}

/// Read the total number of sectors on the block device.
pub fn get_capacity() -> u64 {
    let base = VIRTIO_BLK_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0;
    }
    unsafe {
        let cap_low = core::ptr::read_volatile((base + 0x100) as *const u32) as u64;
        let cap_high = core::ptr::read_volatile((base + 0x104) as *const u32) as u64;
        cap_low | (cap_high << 32)
    }
}
