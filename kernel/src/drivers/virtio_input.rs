//! # 🖱️ Virtio-Input MMIO Physical Mouse / Tablet Driver
//!
//! Parses and handles raw event packets from the physical host mouse/tablet
//! emulated via Virtio-Input.

use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::arch::ArchHardware;
use crate::arch::aarch64::phys::{alloc_page, PageTag, PhysAddr};
use crate::arch::mmu_facade::pa_to_kernel_va;

static VIRTIO_INPUT_BASE: AtomicUsize = AtomicUsize::new(0);

const QUEUE_SIZE: usize = 16;

#[repr(C, align(16))]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

#[repr(C)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; QUEUE_SIZE],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtioInputEvent {
    pub r#type: u16,  // 1 = EV_KEY, 2 = EV_REL, 3 = EV_ABS
    pub code: u16,    // 0 = REL_X/ABS_X, 1 = REL_Y/ABS_Y, 272 = BTN_LEFT, etc.
    pub value: u32,   // position, movement delta, or 1=pressed/0=released
}

// Globals for Virtqueues
static mut DESC_TABLE: *mut VirtqDesc = core::ptr::null_mut();
static mut AVAIL_RING: *mut u16 = core::ptr::null_mut();
static mut USED_RING: *mut u16 = core::ptr::null_mut();
static mut AVAIL_IDX: u16 = 0;
static mut USED_IDX: u16 = 0;

// Host writes events here
static mut EVENT_BUFFERS: *mut VirtioInputEvent = core::ptr::null_mut();

// Global mouse state
pub static mut MOUSE_X: i32 = 200;
pub static mut MOUSE_Y: i32 = 150;
pub static mut MOUSE_DOWN: bool = false;

unsafe fn flush_cache(va: usize, len: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        <crate::arch::CurrentArch as crate::arch::ArchHardware>::clean_and_invalidate_cache_range(va, len);
    }
}

/// Initialize and probe Virtio-MMIO Input devices starting from 0x0a000000.
pub fn init() {
    for i in 0..32 {
        let base = 0x0a000000 + i * 0x200;
        let magic = unsafe { core::ptr::read_volatile(base as *const u32) };
        let dev_id = unsafe { core::ptr::read_volatile((base + 0x008) as *const u32) };
        if magic == 0x74726976 {
            // Diagnostic logging of all Virtio MMIO slots to serial console
            crate::log_info!("VIRTIO_PROBE", "Slot {} MMIO {:#x}: magic=0x{:x}, dev_id={}", i, base, magic, dev_id);
            if dev_id == 18 { // 18 = Input Device
                crate::log_info!("VIRTIO", "Discovered Virtio-Input device at slot {} MMIO {:#x}", i, base);
                match unsafe { init_input_device(base) } {
                    Ok(()) => {
                        VIRTIO_INPUT_BASE.store(base, Ordering::SeqCst);
                        crate::log_info!("VIRTIO", "Virtio-Input device at {:#x} successfully initialized!", base);
                        break;
                    }
                    Err(e) => {
                        crate::log_error!("VIRTIO", "Failed to initialize Virtio-Input at {:#x}: {:?}", base, e);
                    }
                }
            }
        }
    }
}

unsafe fn init_input_device(base: usize) -> Result<()> {
    // 1. Reset device
    core::ptr::write_volatile(base as *mut u32, 0);

    // 2. Set status to ACKNOWLEDGE and DRIVER
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2);

    // 3. Negotiate features (accept all defaults)
    core::ptr::write_volatile((base + 0x020) as *mut u32, 0); // Driver features select
    core::ptr::write_volatile((base + 0x024) as *mut u32, 0); // Driver features write

    // 4. Set FEATURES_OK
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8);

    // 5. Select Event Queue (Queue 0)
    core::ptr::write_volatile((base + 0x030) as *mut u32, 0);

    // Allocate 1 page for Descriptor table + Avail ring + Used ring
    let q_page = alloc_page(PageTag::KernelHeap)?;
    let q_kva = pa_to_kernel_va(q_page.as_usize());
    core::ptr::write_bytes(q_kva as *mut u8, 0, 4096);

    DESC_TABLE = q_kva as *mut VirtqDesc;
    AVAIL_RING = (q_kva + 512) as *mut u16;
    USED_RING = (q_kva + 1024) as *mut u16;

    // Set Queue Address in MMIO
    core::ptr::write_volatile((base + 0x038) as *mut u32, 4096); // Align
    core::ptr::write_volatile((base + 0x040) as *mut u32, (q_page.as_usize() / 4096) as u32);

    // Allocate 1 page for Event Buffers
    let b_page = alloc_page(PageTag::KernelHeap)?;
    let b_kva = pa_to_kernel_va(b_page.as_usize());
    core::ptr::write_bytes(b_kva as *mut u8, 0, 4096);
    EVENT_BUFFERS = b_kva as *mut VirtioInputEvent;

    // Populate descriptors to let the host write events (Descriptor write flags enabled)
    for i in 0..QUEUE_SIZE {
        let buf_addr = b_page.as_usize() + i * core::mem::size_of::<VirtioInputEvent>();
        *DESC_TABLE.add(i) = VirtqDesc {
            addr: buf_addr as u64,
            len: core::mem::size_of::<VirtioInputEvent>() as u32,
            flags: VIRTQ_DESC_F_WRITE, // Host writes
            next: 0,
        };

        // Put in Avail Ring
        *AVAIL_RING.add(2 + i) = i as u16;
    }

    *AVAIL_RING.add(0) = 0; // flags
    *AVAIL_RING.add(1) = QUEUE_SIZE as u16; // idx
    AVAIL_IDX = QUEUE_SIZE as u16;

    // Flush rings to RAM
    flush_cache(q_kva, 4096);
    flush_cache(b_kva, 4096);

    // Set DRIVER_OK status
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 8 | 4);

    // Notify device that Queue 0 is populated
    core::ptr::write_volatile((base + 0x050) as *mut u32, 0);

    Ok(())
}

/// Polls and reads pending Virtio-Input events from the host, updating global coordinates.
pub fn poll_events() {
    let base = VIRTIO_INPUT_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }

    unsafe {
        // Read used ring index
        let used_kva = USED_RING as usize;
        flush_cache(used_kva, 4096);

        let used_ptr = USED_RING as *const VirtqUsed;
        let used_idx = core::ptr::read_volatile(&(*used_ptr).idx);

        if USED_IDX != used_idx {
            // Process new events written by host
            let count = used_idx.wrapping_sub(USED_IDX) as usize;
            for k in 0..count {
                let ring_slot = (USED_IDX.wrapping_add(k as u16) as usize) % QUEUE_SIZE;
                // Read used element desc index in a 100% type-safe way
                let desc_idx = core::ptr::read_volatile(&(*used_ptr).ring[ring_slot].id) as usize;

                // Read event
                let ev_ptr = EVENT_BUFFERS.add(desc_idx);
                let ev = core::ptr::read_volatile(ev_ptr);

                // Parse standard Input Event
                match ev.r#type {
                    1 => { // EV_KEY
                        if ev.code == 272 { // BTN_LEFT
                            MOUSE_DOWN = ev.value != 0;
                        }
                    }
                    2 => { // EV_REL
                        let screen_w = crate::drivers::virtio_gpu::SCREEN_WIDTH as i32;
                        let screen_h = crate::drivers::virtio_gpu::SCREEN_HEIGHT as i32;
                        if ev.code == 0 { // REL_X
                            MOUSE_X = (MOUSE_X + ev.value as i32).clamp(0, screen_w - 1);
                        } else if ev.code == 1 { // REL_Y
                            MOUSE_Y = (MOUSE_Y + ev.value as i32).clamp(0, screen_h - 1);
                        }
                    }
                    3 => { // EV_ABS
                        let screen_w = crate::drivers::virtio_gpu::SCREEN_WIDTH as i32;
                        let screen_h = crate::drivers::virtio_gpu::SCREEN_HEIGHT as i32;
                        if ev.code == 0 { // ABS_X
                            // ABS_X values are in range [0, 32767]
                            MOUSE_X = ((ev.value as i32 * screen_w) / 32768).clamp(0, screen_w - 1);
                        } else if ev.code == 1 { // ABS_Y
                            // ABS_Y values are in range [0, 32767]
                            MOUSE_Y = ((ev.value as i32 * screen_h) / 32768).clamp(0, screen_h - 1);
                        }
                    }
                    _ => {}
                }

                // Recycle descriptor back to avail ring
                *AVAIL_RING.add(2 + ring_slot) = desc_idx as u16;
            }

            USED_IDX = used_idx;

            // Notify device that we recycled the slots
            *AVAIL_RING.add(1) = AVAIL_IDX;
            flush_cache(USED_RING as usize, 4096);
            flush_cache(AVAIL_RING as usize, 4096);

            core::ptr::write_volatile((base + 0x050) as *mut u32, 0); // QueueNotify
        }
    }
}

pub fn get_mouse_x() -> i32 {
    unsafe { MOUSE_X }
}

pub fn get_mouse_y() -> i32 {
    unsafe { MOUSE_Y }
}

pub fn is_mouse_down() -> bool {
    unsafe { MOUSE_DOWN }
}
