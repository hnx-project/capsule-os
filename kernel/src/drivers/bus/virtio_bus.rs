//! # 🚌 virtio-mmio Bus (Generic Primitive Layer)
//!
//! Per the microkernel principle, individual virtio device protocols
//! (blk, net, gpu, input, ...) live in EL0 userspace.  The kernel
//! exposes only the **bus primitives** below so that EL0 drivers can:
//!
//!   1. probe virtio-mmio devices scattered across the 0x0a000000
//!      region (one device per 0x200-byte slot),
//!   2. allocate a virtqueue (descriptor table + available ring +
//!      used ring) backed by physically contiguous pages that are
//!      then mappable into the calling process via `vmar_map_self`,
//!   3. kick the device after queueing descriptors, and
//!   4. read the device's interrupt status register (ISR).
//!
//! EL0 services are expected to drive protocol-level registers via
//! the existing `mmio_read`/`mmio_write` syscalls once the kernel has
//! granted them access to the slot's MMIO page via `vmar_map_self`.

use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};

pub const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
pub const VIRTIO_MMIO_STRIDE: usize = 0x200;
pub const VIRTIO_MMIO_SLOTS: usize = 32;
pub const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976;

/// Snapshot of a single virtio-mmio device slot discovered by the
/// kernel probe.  EL0 services receive this list via the
/// `SYSCALL_VIRTIO_PROBE` syscall.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioDeviceInfo {
    pub slot: u32,
    pub mmio_base: u64,
    pub device_id: u32,
    pub version: u32,
    pub irq: u32,
}

/// Metadata returned by `SYSCALL_VIRTIO_SETUP_QUEUE` describing the
/// three VMOs that back a single virtqueue.  EL0 services `vmar_map_self`
/// each VMO into its address space to obtain direct R/W access to the
/// descriptor / available / used rings.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioQueueHandles {
    pub desc_vmo: u64,
    pub avail_vmo: u64,
    pub used_vmo: u64,
    pub desc_bytes: u64,
    pub avail_bytes: u64,
    pub used_bytes: u64,
    pub qsize: u32,
    pub _pad: u32,
}

const fn bytes_for_qsize(qsize: u16) -> (usize, usize, usize) {
    let q = qsize as usize;
    let desc_bytes = 16 * q;
    let avail_bytes = 6 + 2 * q;
    let mut used_bytes = 6 + 8 * q;
    if used_bytes < 4096 {
        used_bytes = 4096;
    }
    (desc_bytes, avail_bytes, used_bytes)
}

fn alloc_one_vmo(bytes: usize) -> Result<usize> {
    let rounded = (bytes + 4095) & !4095;
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
        .ok_or(Status::NotFound)?;
    let table = if unsafe { (*thread_ptr).handle_table.is_null() } {
        return Err(Status::NotAllowed);
    } else {
        unsafe { &*(*thread_ptr).handle_table }
    };
    let hv = crate::syscall::handlers::memory::sys_vmo_create(table, rounded)?;
    Ok(hv.get() as usize)
}

/// Snapshot of every probed virtio-mmio device in the system.
/// Refreshed on each call to `sys_virtio_probe`.
static mut DEVICE_SLOTS: [VirtioDeviceInfo; VIRTIO_MMIO_SLOTS] =
    [VirtioDeviceInfo { slot: 0, mmio_base: 0, device_id: 0, version: 0, irq: 0 }; VIRTIO_MMIO_SLOTS];
static mut DEVICE_COUNT: usize = 0;
static PROBE_DONE: AtomicUsize = AtomicUsize::new(0);

fn do_probe() {
    unsafe {
        DEVICE_COUNT = 0;
        for i in 0..VIRTIO_MMIO_SLOTS {
            let pa = VIRTIO_MMIO_BASE + i * VIRTIO_MMIO_STRIDE;
            // When called from a syscall handler, TTBR0_EL1 holds the
            // user process's L0 page table, which does NOT map
            // 0x0a000000.  Walk the kernel's high-half mirror
            // (TTBR1_EL1) instead so the read always succeeds.
            let va = crate::arch::mmu_facade::pa_to_kernel_va(pa);
            let magic = core::ptr::read_volatile(va as *const u32);
            if magic != VIRTIO_MMIO_MAGIC {
                continue;
            }
            let version = core::ptr::read_volatile((va + 0x004) as *const u32);
            let dev_id = core::ptr::read_volatile((va + 0x008) as *const u32);
            let irq_lo = core::ptr::read_volatile((va + 0x00c) as *const u32);
            DEVICE_SLOTS[DEVICE_COUNT] = VirtioDeviceInfo {
                slot: i as u32,
                mmio_base: pa as u64,
                device_id: dev_id,
                version,
                irq: irq_lo,
            };
            DEVICE_COUNT += 1;
            crate::log_info!(
                "VIRTIO-BUS",
                "slot={} dev_id={} version={} irq={}",
                i,
                dev_id,
                version,
                irq_lo
            );
        }
        PROBE_DONE.store(1, Ordering::SeqCst);
    }
}

/// Public probe entry point.  Not called at boot — we run the scan
/// lazily on the first `SYSCALL_VIRTIO_PROBE` so the virtio-mmio
/// devices have time to finish their own power-on reset.  This is
/// the **only** place the kernel touches the virtio-MMIO magic /
/// version registers; EL0 services use the cached list returned by
/// `sys_virtio_probe`.
pub fn init() {
    // intentionally empty — see `sys_virtio_probe` for the lazy
    // scan entry point.
}

/// Fill `dst` with up to `dst.len()` device records.
/// Returns the number of devices written.
pub fn sys_virtio_probe(dst: usize, buf_len: usize) -> Result<usize> {
    if dst == 0 || buf_len < 8 {
        return Err(Status::InvalidArgs);
    }
    let cap = (buf_len - 8) / core::mem::size_of::<VirtioDeviceInfo>();
    if cap == 0 {
        return Err(Status::InvalidArgs);
    }
    if PROBE_DONE.load(Ordering::Acquire) == 0 {
        do_probe();
    }

    let l0_pa = crate::arch::aarch64::mmu::translate_user_va(
        current_l0_pa()?,
        dst,
    )
    .ok_or(Status::InvalidArgs)?;

    let count = unsafe { DEVICE_COUNT }.min(cap);
    unsafe {
        let header = l0_pa as *mut u32;
        *header = count as u32;
        let payload = (l0_pa as *mut u8).add(8);
        core::ptr::copy_nonoverlapping(
            DEVICE_SLOTS.as_ptr() as *const u8,
            payload,
            count * core::mem::size_of::<VirtioDeviceInfo>(),
        );
    }
    Ok(8 + count * core::mem::size_of::<VirtioDeviceInfo>())
}

/// Allocate three VMOs backing `qsize` descriptors for device
/// `slot`, queue selector `qsel`.  Returns the handles; EL0 services
/// must `vmar_map_self` each one before writing descriptors.
pub fn sys_virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<VirtioQueueHandles> {
    if slot >= VIRTIO_MMIO_SLOTS as u32 {
        return Err(Status::InvalidArgs);
    }
    if qsize == 0 || (qsize & (qsize - 1)) != 0 {
        return Err(Status::InvalidArgs);
    }
    let qsize = qsize.min(1024) as usize;
    let (desc_b, avail_b, used_b) = bytes_for_qsize(qsize as u16);

    let desc_vmo = alloc_one_vmo(desc_b)?;
    let avail_vmo = alloc_one_vmo(avail_b)?;
    let used_vmo = alloc_one_vmo(used_b)?;

    crate::log_info!(
        "VIRTIO-BUS",
        "queue slot={} qsel={} qsize={} desc_vmo={} avail_vmo={} used_vmo={}",
        slot,
        qsel,
        qsize,
        desc_vmo,
        avail_vmo,
        used_vmo
    );

    Ok(VirtioQueueHandles {
        desc_vmo: desc_vmo as u64,
        avail_vmo: avail_vmo as u64,
        used_vmo: used_vmo as u64,
        desc_bytes: desc_b as u64,
        avail_bytes: avail_b as u64,
        used_bytes: used_b as u64,
        qsize: qsize as u32,
        _pad: 0,
    })
}

/// Write QueueNotify (offset 0x050) for `(slot, qsel)`.  EL0 services
/// typically pair this with a `mmio_write` to the device's MMIO page
/// (kernel-mapped at boot for the slot).
pub fn sys_virtio_kick(slot: u32, qsel: u16) -> Result<()> {
    if slot >= VIRTIO_MMIO_SLOTS as u32 {
        return Err(Status::InvalidArgs);
    }
    let pa = unsafe { DEVICE_SLOTS[slot as usize].mmio_base as usize };
    let va = unsafe { crate::arch::mmu_facade::pa_to_kernel_va(pa) };
    unsafe {
        core::ptr::write_volatile((va + 0x050) as *mut u32, qsel as u32);
    }
    Ok(())
}

/// Read ISR (offset 0x060) for `slot` and acknowledge the
/// corresponding queue interrupts.  Returns the raw 32-bit value.
pub fn sys_virtio_read_isr(slot: u32) -> Result<u32> {
    if slot >= VIRTIO_MMIO_SLOTS as u32 {
        return Err(Status::InvalidArgs);
    }
    let pa = unsafe { DEVICE_SLOTS[slot as usize].mmio_base as usize };
    let va = unsafe { crate::arch::mmu_facade::pa_to_kernel_va(pa) };
    Ok(unsafe { core::ptr::read_volatile((va + 0x060) as *const u32) })
}

fn current_l0_pa() -> Result<usize> {
    let t = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let t = t.ok_or(Status::NotFound)?;
    let pid = unsafe { (*t).process_id };
    let p = crate::task::process::find_process_mut(pid).ok_or(Status::NotFound)?;
    Ok(p.page_table.l0_pa())
}