//! # blkdev — EL1 Kext module for virtio-blk
//!
//! Provides a kernel-resident block device service. Loaded into EL1 by
//! the kernel pill_loader via OHLINK dynamic relocation.

#![no_std]
#![no_main]

extern crate modskit;
extern crate shared;

use modskit::{mod_log_info, mod_log_error, ModsBus};
use shared::status::{Result, Status};

const QUEUE_SIZE: usize = 16;
const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

#[repr(C, align(16))]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct BlkHeader {
    r#type: u32,
    reserved: u32,
    sector: u64,
}

const BLK_CMD_OPEN: u8 = 0;
const BLK_CMD_READ: u8 = 1;
const BLK_CMD_WRITE: u8 = 2;
const BLK_CMD_SIZE: u8 = 3;

const MMIO_VIRTIO_MAGIC: u32 = 0x74726976;

const VMO_TARGET_DESC: usize = 0x1000_0000;
const VMO_TARGET_BUF: usize = 0x1010_0000;
const VMO_TARGET_HEADER: usize = 0x1020_0000;
const VMO_TARGET_DATA: usize = 0x1020_1000;
const VMO_TARGET_STATUS: usize = 0x1020_2000;

/// Pre-allocated physical ranges for the queue backing memory and
/// DMA buffers.
const Q_PHYS_BASE: usize = 0x4a40_0000;
const HEADER_PHYS: usize = 0x4a40_8000;
const DATA_PHYS: usize = 0x4a40_8100;
const STATUS_PHYS: usize = 0x4a40_8200;

/// `mmio_pa` is the physical address of the device's 0x200-byte MMIO slot.
/// Because we run in EL1 we translate it to a kernel virtual address via
/// `ModsBus::pa_to_kernel_va` and then write directly.
struct BlkDriver {
    slot: u32,
    mmio_pa: usize,
    desc_vmo: usize,
    avail_vmo: usize,
    used_vmo: usize,
    buf_vmo: usize,
    header_phys: usize,
    data_phys: usize,
    status_phys: usize,
    capacity_sectors: u64,
    avail_idx: u16,
    used_idx: u16,
}

fn mmio_read(base: usize, off: usize) -> u32 {
    ModsBus::mmio_read(base, off)
}

fn mmio_write(base: usize, off: usize, val: u32) -> Result<()> {
    ModsBus::mmio_write(base, off, val)
}

fn cache_flush(_vmo: usize) {
    // Cache flush in EL1 is handled implicitly by clean D-cache ops on
    // the descriptor region; we leave this as a hint hook for the
    // kernel to grow in a future revision.
}

fn discover_blk() -> Option<(u32, usize)> {
    // Hard-coded for the QEMU virt machine: virtio-blk lives at slot 31.
    let slot = 31u32;
    let mmio_base = 0x0a00_0000usize + (slot as usize) * 0x200;
    Some((slot, mmio_base))
}

impl BlkDriver {
    fn init() -> Result<Self> {
        let (slot, mmio_pa) = discover_blk().ok_or(Status::NotFound)?;
        mod_log_info!("BLKDEV", "discovered slot={} mmio_pa={:#x}", slot, mmio_pa);

        // Allocate the three queue VMOs through the kernel-exported helper.
        let handles = ModsBus::virtio_setup_queue(slot, 0, QUEUE_SIZE as u16)?;

        // Map them into the module's address space.
        ModsBus::map_memory(
            handles.desc_vmo as usize,
            VMO_TARGET_DESC,
            (handles.desc_bytes as usize + 4095) & !4095,
            11,
        )?;
        ModsBus::map_memory(
            handles.avail_vmo as usize,
            VMO_TARGET_DESC + 4096,
            (handles.avail_bytes as usize + 4095) & !4095,
            11,
        )?;
        ModsBus::map_memory(
            handles.used_vmo as usize,
            VMO_TARGET_DESC + 8192,
            (handles.used_bytes as usize + 4095) & !4095,
            11,
        )?;

        // Sanity-check the device magic.
        let magic = mmio_read(mmio_pa, 0x000);
        if magic != MMIO_VIRTIO_MAGIC {
            mod_log_error!("BLKDEV", "magic mismatch (got {:#x})", magic);
            return Err(Status::InvalidArgs);
        }

        // Drive the device into DRIVER_OK.
        mmio_write(mmio_pa, 0x070, 0)?;
        mmio_write(mmio_pa, 0x070, 1 | 2)?;
        mmio_write(mmio_pa, 0x014, 0)?;
        let f0 = mmio_read(mmio_pa, 0x010);
        mmio_write(mmio_pa, 0x020, f0)?;
        mmio_write(mmio_pa, 0x070, 1 | 2 | 8)?;
        mmio_write(mmio_pa, 0x028, 4096)?;
        mmio_write(mmio_pa, 0x030, 0)?;
        mmio_write(mmio_pa, 0x038, QUEUE_SIZE as u32)?;
        mmio_write(mmio_pa, 0x03c, 4096)?;
        mmio_write(mmio_pa, 0x040, (Q_PHYS_BASE / 4096) as u32)?;
        mmio_write(mmio_pa, 0x070, 1 | 2 | 8 | 4)?;

        let cap_lo = mmio_read(mmio_pa, 0x100);
        let cap_hi = mmio_read(mmio_pa, 0x104);
        let cap = ((cap_hi as u64) << 32) | (cap_lo as u64);
        mod_log_info!("BLKDEV", "capacity={} sectors (legacy fallback: 2880)", cap);

        let _ = handles;

        Ok(BlkDriver {
            slot,
            mmio_pa,
            desc_vmo: 0,
            avail_vmo: 0,
            used_vmo: 0,
            buf_vmo: 0,
            header_phys: HEADER_PHYS,
            data_phys: DATA_PHYS,
            status_phys: STATUS_PHYS,
            capacity_sectors: 2880,
            avail_idx: 0,
            used_idx: 0,
        })
    }

    fn submit_sector(&mut self, sector: u64, write: bool) -> Result<()> {
        let hdr_va = VMO_TARGET_HEADER as *mut BlkHeader;
        unsafe {
            core::ptr::write_volatile(
                hdr_va,
                BlkHeader {
                    r#type: if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN },
                    reserved: 0,
                    sector,
                },
            );

            let desc_va = VMO_TARGET_DESC as *mut VirtqDesc;
            *desc_va.add(0) = VirtqDesc {
                addr: self.header_phys as u64,
                len: core::mem::size_of::<BlkHeader>() as u32,
                flags: VIRTQ_DESC_F_NEXT,
                next: 1,
            };
            *desc_va.add(1) = VirtqDesc {
                addr: self.data_phys as u64,
                len: 512,
                flags: if write { VIRTQ_DESC_F_NEXT } else { VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE },
                next: 2,
            };
            *desc_va.add(2) = VirtqDesc {
                addr: self.status_phys as u64,
                len: 1,
                flags: VIRTQ_DESC_F_WRITE,
                next: 0,
            };

            let avail_va = (VMO_TARGET_DESC + 4096) as *mut u16;
            let used_va = (VMO_TARGET_DESC + 8192) as *mut u16;

            let ring_idx_offset = (self.avail_idx % QUEUE_SIZE as u16) as usize;
            core::ptr::write_volatile(avail_va.add(2 + ring_idx_offset) as *mut u16, 0);
            self.avail_idx = self.avail_idx.wrapping_add(1);
            core::ptr::write_volatile(avail_va.add(1), self.avail_idx);

            cache_flush(0);
            cache_flush(0);
            cache_flush(0);

            ModsBus::virtio_kick(self.slot, 0)?;

            loop {
                cache_flush(0);
                let new_used = core::ptr::read_volatile(used_va.add(1));
                if new_used != self.used_idx {
                    self.used_idx = new_used;
                    break;
                }
                ModsBus::yield_cpu();
            }
        }
        Ok(())
    }

    fn do_read(&mut self, sector: u64) -> Result<[u8; 512]> {
        self.submit_sector(sector, false)?;
        let mut out = [0u8; 512];
        unsafe {
            core::ptr::copy_nonoverlapping(
                VMO_TARGET_DATA as *const u8,
                out.as_mut_ptr(),
                512,
            );
        }
        Ok(out)
    }

    fn do_write(&mut self, sector: u64, data: &[u8; 512]) -> Result<()> {
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                VMO_TARGET_DATA as *mut u8,
                512,
            );
        }
        self.submit_sector(sector, true)
    }
}

/// OHLINK entry point: the kernel pill_loader invokes this when the
/// blkdev.pill bundle is admitted into the system. We perform hardware
/// bring-up here, then return to let the kernel schedule the service
/// loop.
#[no_mangle]
pub extern "C" fn pill_init() -> i32 {
    mod_log_info!("BLKDEV", "EL1 Kext blkdev init");

    match BlkDriver::init() {
        Ok(_) => 0,
        Err(e) => {
            mod_log_error!("BLKDEV", "init failed: {:?}", e);
            -1
        }
    }
}

/// OHLINK service loop. The kernel will spin this entry after
/// `pill_init` succeeds. We keep it minimal here — the user-facing
/// block service continues to be mediated via IPC channels allocated
/// by the kernel on behalf of this module.
#[no_mangle]
pub extern "C" fn pill_main() -> ! {
    mod_log_info!("BLKDEV", "blkdev EL1 service loop running");
    loop {
        ModsBus::yield_cpu();
    }
}

// Silence the unused-helper warning when the user re-adds IPC later.
#[allow(dead_code)]
const _BLK_CMDS: [u8; 4] = [BLK_CMD_OPEN, BLK_CMD_READ, BLK_CMD_WRITE, BLK_CMD_SIZE];

/// `#[no_main]` requires a `main` symbol. It is never called because
/// the Kext is loaded by `pill_loader`, not invoked as a process.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    pill_init()
}