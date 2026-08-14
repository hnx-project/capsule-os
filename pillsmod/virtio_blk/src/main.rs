#![no_std]
#![no_main]

use libpillsmod::{pill_print, KernelImportTable, BlockDeviceOps};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Helper to translate physical address to high-half kernel virtual address in EL1.
fn pa_to_kernel_va(pa: usize) -> usize {
    pa.wrapping_add(0xFFFF_8000_0000_0000)
}

/// The selected Virtio MMIO block device base address. 0 means not found/uninitialized.
static VIRTIO_BLK_BASE: AtomicUsize = AtomicUsize::new(0);
static DISK_CAPACITY: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

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

/// Static instance of the block device callbacks.
static BLOCK_DEVICE_OPS: BlockDeviceOps = BlockDeviceOps {
    read_sectors,
    write_sectors,
    get_capacity,
};

#[no_mangle]
#[link_section = ".entry"]
pub extern "C" fn pillsmod_init(kernel: &KernelImportTable) -> i32 {
    pill_print(kernel, "Virtio-Blk PillsMod loaded! Probing MMIO bus slot...");

    // Probe 32 MMIO slots starting at 0x0a000000
    for i in 0..32 {
        let base_pa = 0x0a000000 + i * 0x200;
        let base_va = pa_to_kernel_va(base_pa);
        let magic = unsafe { core::ptr::read_volatile(base_va as *const u32) };
        let dev_id = unsafe { core::ptr::read_volatile((base_va + 0x008) as *const u32) };

        if magic == 0x74726976 { // "virt"
            if dev_id == 2 { // Block Device
                pill_print(kernel, "Discovered Virtio-Blk device! Initializing...");
                match unsafe { init_device(kernel, base_va) } {
                    Ok(capacity) => {
                        VIRTIO_BLK_BASE.store(base_va, Ordering::SeqCst);
                        DISK_CAPACITY.store(capacity, Ordering::SeqCst);
                        
                        // Register this driver back to the kernel
                        let ret = (kernel.register_block_device)(&BLOCK_DEVICE_OPS);
                        if ret == 0 {
                            pill_print(kernel, "Virtio-Blk driver successfully initialized and registered as ACTIVE_BLOCK_DEVICE!");
                            return 0;
                        } else {
                            pill_print(kernel, "Failed to register block device with kernel!");
                            return -1;
                        }
                    }
                    Err(e) => {
                        pill_print(kernel, "Failed to initialize Virtio-Blk device!");
                        return e;
                    }
                }
            }
        }
    }

    pill_print(kernel, "No Virtio-Blk device found on MMIO slots.");
    -1
}

#[no_mangle]
pub extern "C" fn pillsmod_exit(_kernel: &KernelImportTable) -> i32 {
    0
}

/// Legacy Virtio MMIO Block Device Initialization.
unsafe fn init_device(kernel: &KernelImportTable, base: usize) -> Result<u64, i32> {
    // 1. Reset device
    core::ptr::write_volatile((base + 0x070) as *mut u32, 0); // Status = 0

    // 2. Acknowledge & Driver status bits
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1); // Status = ACKNOWLEDGE
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2); // Status = ACKNOWLEDGE | DRIVER

    // 3. Negotiate features
    core::ptr::write_volatile((base + 0x014) as *mut u32, 0); // DeviceFeaturesSel = 0
    let f0 = core::ptr::read_volatile((base + 0x010) as *const u32);
    
    // Minimal block features: SizeMax (bit 2), Geometry (bit 9), BlockSize (bit 11)
    let negotiated = f0 & ((1 << 2) | (1 << 9) | (1 << 11));
    core::ptr::write_volatile((base + 0x024) as *mut u32, 0); // DriverFeaturesSel = 0
    core::ptr::write_volatile((base + 0x020) as *mut u32, negotiated);
    core::ptr::write_volatile((base + 0x024) as *mut u32, 1); // DriverFeaturesSel = 1
    core::ptr::write_volatile((base + 0x020) as *mut u32, 0);

    // Set FEATURES_OK status bit
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8); // ACKNOWLEDGE | DRIVER | FEATURES_OK
    let features_ok_status = core::ptr::read_volatile((base + 0x070) as *const u32);
    if (features_ok_status & 8) == 0 {
        return Err(-101);
    }

    // Set GuestPageSize (0x028) to 4096
    core::ptr::write_volatile((base + 0x028) as *mut u32, 4096);

    // 4. Configure Virtqueue 0
    core::ptr::write_volatile((base + 0x030) as *mut u32, 0); // QueueSel = 0
    let max_size = core::ptr::read_volatile((base + 0x034) as *const u32);
    if max_size < QUEUE_SIZE as u32 {
        return Err(-102);
    }
    core::ptr::write_volatile((base + 0x038) as *mut u32, QUEUE_SIZE as u32); // QueueNum = QUEUE_SIZE
    core::ptr::write_volatile((base + 0x03c) as *mut u32, 4096); // QueueAlign = 4096

    // 5. Allocate 2 contiguous physical pages
    let page0 = (kernel.alloc_pages)(2);
    if page0 == 0 {
        return Err(-103);
    }

    // Zero initialize pages
    let kva0 = pa_to_kernel_va(page0 as usize);
    core::ptr::write_bytes(kva0 as *mut u8, 0, 8192);

    DESC_TABLE = kva0 as *mut VirtqDesc;
    AVAIL_RING = (kva0 + 256) as *mut u16;
    USED_RING = (kva0 + 4096) as *mut u16;

    // Allocate dedicated buffers for request headers and status byte
    let buf_page = (kernel.alloc_pages)(1);
    if buf_page == 0 {
        (kernel.free_pages)(page0, 2);
        return Err(-104);
    }
    let buf_kva = pa_to_kernel_va(buf_page as usize);
    core::ptr::write_bytes(buf_kva as *mut u8, 0, 4096);
    BLK_HEADER = buf_kva as *mut VirtIOBlkHeader;
    BLK_STATUS = (buf_kva + 1024) as *mut u8;

    // Tell the device where the queue is located (PFN)
    core::ptr::write_volatile((base + 0x040) as *mut u32, (page0 >> 12) as u32); // QueuePFN

    // 6. Set DRIVER_OK status
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8 | 4);

    // Read and log disk capacity
    let cap_low = core::ptr::read_volatile((base + 0x100) as *const u32) as u64;
    let cap_high = core::ptr::read_volatile((base + 0x104) as *const u32) as u64;
    let capacity = cap_low | (cap_high << 32);

    Ok(capacity)
}

extern "C" fn read_sectors(sector: u64, dst_pa: usize) -> i32 {
    let base = VIRTIO_BLK_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return -1;
    }

    unsafe {
        // Construct the request header
        core::ptr::write_volatile(BLK_HEADER, VirtIOBlkHeader {
            r#type: VIRTIO_BLK_T_IN,
            reserved: 0,
            sector,
        });
        core::ptr::write_volatile(BLK_STATUS, 0xFF); // Initialize status

        // Setup descriptor table entries
        // Descriptor 0: request header (Read-Only)
        let header_pa = pa_to_kernel_va(BLK_HEADER as usize) - 0xFFFF_8000_0000_0000;
        core::ptr::write_volatile(&mut (*DESC_TABLE.add(0)), VirtqDesc {
            addr: header_pa as u64,
            len: core::mem::size_of::<VirtIOBlkHeader>() as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        });

        // Descriptor 1: data buffer (Write-Only to Guest RAM)
        core::ptr::write_volatile(&mut (*DESC_TABLE.add(1)), VirtqDesc {
            addr: dst_pa as u64,
            len: 512,
            flags: VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE,
            next: 2,
        });

        // Descriptor 2: status byte (Write-Only to Guest RAM)
        let status_pa = pa_to_kernel_va(BLK_STATUS as usize) - 0xFFFF_8000_0000_0000;
        core::ptr::write_volatile(&mut (*DESC_TABLE.add(2)), VirtqDesc {
            addr: status_pa as u64,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        });

        // Publish descriptors via the Available Ring
        let avail_head_idx = (AVAIL_IDX % QUEUE_SIZE as u16) as usize;
        core::ptr::write_volatile(AVAIL_RING.add(2 + avail_head_idx), 0); // Put head desc index (0) in ring

        AVAIL_IDX = AVAIL_IDX.wrapping_add(1);
        core::ptr::write_volatile(AVAIL_RING.add(1), AVAIL_IDX); // Update avail idx

        // Notify device of queue 0 activity
        core::ptr::write_volatile((base + 0x050) as *mut u32, 0); // QueueNotify = 0

        // Poll the Used Ring for completion
        let expected_idx = USED_IDX;
        loop {
            let current_idx = core::ptr::read_volatile(USED_RING.add(1));
            if current_idx != expected_idx {
                break;
            }
            core::hint::spin_loop();
        }
        USED_IDX = USED_IDX.wrapping_add(1);

        let status = core::ptr::read_volatile(BLK_STATUS);
        if status == 0 {
            0 // Success
        } else {
            -2
        }
    }
}

extern "C" fn write_sectors(sector: u64, src_pa: usize) -> i32 {
    let base = VIRTIO_BLK_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return -1;
    }

    unsafe {
        // Construct the request header
        core::ptr::write_volatile(BLK_HEADER, VirtIOBlkHeader {
            r#type: VIRTIO_BLK_T_OUT,
            reserved: 0,
            sector,
        });
        core::ptr::write_volatile(BLK_STATUS, 0xFF);

        // Setup descriptor table entries
        // Descriptor 0: request header (Read-Only)
        let header_pa = pa_to_kernel_va(BLK_HEADER as usize) - 0xFFFF_8000_0000_0000;
        core::ptr::write_volatile(&mut (*DESC_TABLE.add(0)), VirtqDesc {
            addr: header_pa as u64,
            len: core::mem::size_of::<VirtIOBlkHeader>() as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        });

        // Descriptor 1: data buffer (Read-Only)
        core::ptr::write_volatile(&mut (*DESC_TABLE.add(1)), VirtqDesc {
            addr: src_pa as u64,
            len: 512,
            flags: VIRTQ_DESC_F_NEXT, // Input data read-only for disk write
            next: 2,
        });

        // Descriptor 2: status byte (Write-Only to Guest RAM)
        let status_pa = pa_to_kernel_va(BLK_STATUS as usize) - 0xFFFF_8000_0000_0000;
        core::ptr::write_volatile(&mut (*DESC_TABLE.add(2)), VirtqDesc {
            addr: status_pa as u64,
            len: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        });

        // Publish descriptors via the Available Ring
        let avail_head_idx = (AVAIL_IDX % QUEUE_SIZE as u16) as usize;
        core::ptr::write_volatile(AVAIL_RING.add(2 + avail_head_idx), 0);

        AVAIL_IDX = AVAIL_IDX.wrapping_add(1);
        core::ptr::write_volatile(AVAIL_RING.add(1), AVAIL_IDX);

        // Notify device of queue 0 activity
        core::ptr::write_volatile((base + 0x050) as *mut u32, 0); // QueueNotify = 0

        // Poll the Used Ring for completion
        let expected_idx = USED_IDX;
        loop {
            let current_idx = core::ptr::read_volatile(USED_RING.add(1));
            if current_idx != expected_idx {
                break;
            }
            core::hint::spin_loop();
        }
        USED_IDX = USED_IDX.wrapping_add(1);

        let status = core::ptr::read_volatile(BLK_STATUS);
        if status == 0 {
            0 // Success
        } else {
            -2
        }
    }
}

extern "C" fn get_capacity() -> u64 {
    DISK_CAPACITY.load(Ordering::Relaxed)
}
