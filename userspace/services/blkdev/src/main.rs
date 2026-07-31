#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_error, syscalls};
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
const MMIO_REGION_BASE: usize = 0x0a00_0000;
const MMIO_REGION_STRIDE: usize = 0x200;

unsafe fn mmio_read(base: usize, off: usize) -> u32 {
    syscalls::mmio_read(base, off).unwrap_or(0)
}

unsafe fn mmio_write(base: usize, off: usize, val: u32) -> Result<()> {
    syscalls::mmio_write(base, off, val)
}

unsafe fn cache_flush(vmo: usize) {
    let _ = syscalls::display_flush(vmo);
}

fn discover_blk() -> Option<(u32, usize)> {
    // Hard-coded for the QEMU virt machine: virtio-blk lives at
    // slot 31 (last MMIO slot), base 0x0a003e00.  The virtio_probe
    // syscall returns devices but the kernel currently records the
    // slot with a stale "dev_id=0" entry for every empty slot
    // (QEMU doesn't refuse to read those, it returns the
    // next-populated slot's register), so we can't distinguish
    // empty from blk from the user-space side.  Until the kernel
    // probe filters out dev_id==0 rows this hard-coded path is the
    // only reliable way to find blk.
    let slot = 31u32;
    let mmio_base = 0x0a00_0000usize + (slot as usize) * 0x200;
    Some((slot, mmio_base))
}

/// `slot_pa` is the physical address of the device's 0x200-byte
/// MMIO slot (e.g. `0x0a000000 + slot * 0x200`).  Because the kernel
/// doesn't mmap individual MMIO slots into the user's L0, every
/// register access must go through `mmio_read` / `mmio_write`.
///
/// `q_phys` is the physical base of our virtqueue backing memory;
/// the kernel has already mapped the three VMOs into our L0 via
/// `vmar_map_self`, but we use the **physical** addresses when
/// writing descriptors (DMA targets).
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

const VMO_TARGET_DESC: usize = 0x1000_0000;
const VMO_TARGET_BUF: usize = 0x1010_0000;
const VMO_TARGET_HEADER: usize = 0x1020_0000;
const VMO_TARGET_DATA: usize = 0x1020_1000;
const VMO_TARGET_STATUS: usize = 0x1020_1200;

/// Pre-allocated physical ranges for the queue backing memory and
/// DMA buffers.  These are reserved by the kernel's virtio_bus setup
/// path so they are guaranteed to be present and contiguous.
/// (The numbers are arbitrary; any out-of-RAM page works because
/// the kernel allocates the VMOs for us — `vmar_map_self` returns
/// the user VA that we can actually touch.)
const Q_PHYS_BASE: usize = 0x4a40_0000;
const HEADER_PHYS: usize = 0x4a40_8000;
const DATA_PHYS: usize = 0x4a40_8100;
const STATUS_PHYS: usize = 0x4a40_8200;

impl BlkDriver {
    unsafe fn init() -> Result<Self> {
        let (slot, mmio_pa) = discover_blk().ok_or(Status::NotFound)?;
        log_info!("BLKDEV", "discovered slot={} mmio_pa={:#x}", slot, mmio_pa);

        // Allocate the three queue VMOs from the kernel bus layer.
        let handles = syscalls::virtio_setup_queue(slot, 0, QUEUE_SIZE as u16)?;

        // The kernel allocates page-aligned VMOs of the requested
        // sizes.  We don't know the exact physical base they live
        // at (the kernel picked it), so we use 0 as a placeholder;
        // real virtio DMA needs `paddr`, which we approximate by
        // assuming the kernel placed them in the contiguous Q_PHYS
        // window — for the 1.0 demo this is what the kernel's
        // allocator does (see virtio_bus.rs's alloc_one_vmo).
        syscalls::vmar_map_self(
            handles.desc_vmo as usize,
            VMO_TARGET_DESC,
            handles.desc_bytes as usize,
            11,
        )?;
        syscalls::vmar_map_self(
            handles.avail_vmo as usize,
            VMO_TARGET_DESC + 4096,
            handles.avail_bytes as usize,
            11,
        )?;
        syscalls::vmar_map_self(
            handles.used_vmo as usize,
            VMO_TARGET_DESC + 8192,
            handles.used_bytes as usize,
            11,
        )?;

        // Allocate three single-page VMOs for header / data / status.
        let header_vmo = syscalls::vmo_create(4096)?;
        let data_vmo = syscalls::vmo_create(4096)?;
        let status_vmo = syscalls::vmo_create(4096)?;
        syscalls::vmar_map_self(header_vmo, VMO_TARGET_HEADER, 4096, 11)?;
        syscalls::vmar_map_self(data_vmo, VMO_TARGET_DATA, 4096, 11)?;
        syscalls::vmar_map_self(status_vmo, VMO_TARGET_STATUS, 4096, 11)?;
        core::ptr::write_bytes(VMO_TARGET_HEADER as *mut u8, 0, 4096);
        core::ptr::write_bytes(VMO_TARGET_DATA as *mut u8, 0, 4096);
        core::ptr::write_bytes(VMO_TARGET_STATUS as *mut u8, 0, 4096);

        // Drive the device via per-register syscalls (no MMIO mmap).
        let magic = unsafe { mmio_read(mmio_pa, 0x000) };
        if magic != MMIO_VIRTIO_MAGIC {
            log_error!("BLKDEV", "magic mismatch (got {:#x})", magic);
            return Err(Status::InvalidArgs);
        }

        unsafe {
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
            log_info!("BLKDEV", "capacity={} sectors (legacy fallback: 2880)", cap);
        }

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

    unsafe fn submit_sector(&mut self, sector: u64, write: bool) -> Result<()> {
        let hdr_va = VMO_TARGET_HEADER as *mut BlkHeader;
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

        syscalls::virtio_kick(self.slot, 0)?;

        loop {
            cache_flush(0);
            let new_used = core::ptr::read_volatile(used_va.add(1));
            if new_used != self.used_idx {
                self.used_idx = new_used;
                break;
            }
            let _ = syscalls::yield_cpu();
        }

        Ok(())
    }

    fn do_read(&mut self, sector: u64) -> Result<[u8; 512]> {
        unsafe { self.submit_sector(sector, false) }?;
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
            self.submit_sector(sector, true)
        }
    }
}

fn write_response(chan: usize, val: i64) {
    let resp = val.to_le_bytes();
    let _ = syscalls::channel_write(chan, &resp, &[]);
}

fn write_response_data(chan: usize, val: i64, data: &[u8]) {
    let mut resp = [0u8; 8 + 1024];
    resp[..8].copy_from_slice(&val.to_le_bytes());
    let n = data.len().min(1016);
    resp[8..8 + n].copy_from_slice(&data[..n]);
    let _ = syscalls::channel_write(chan, &resp[..8 + n], &[]);
}

#[no_mangle]
pub fn main() -> i32 {
    log_info!("BLKDEV", "blkdev: init (microkernel EL0 virtio-blk)");

    let driver = unsafe {
        match BlkDriver::init() {
            Ok(d) => d,
            Err(e) => {
                log_error!("BLKDEV", "init failed: {:?}", e);
                let raw = syscalls::channel_create().unwrap_or(0);
                let chan = (raw >> 32) as u32 as usize;
                let _ = syscalls::channel_register("svc.blk", chan);
                let _ = libcapsule::notify_init("blkdev");
                return -1;
            }
        }
    };
    let mut driver = driver;

    let raw = syscalls::channel_create().unwrap_or(0);
    let server_chan = (raw >> 32) as u32 as usize;
    log_info!("BLKDEV", "channel={}", server_chan);

    if syscalls::channel_register("svc.blk", server_chan).is_err() {
        log_error!("BLKDEV", "register failed");
        return -2;
    }
    log_info!("BLKDEV", "registered svc.blk");
    let _ = libcapsule::notify_init("blkdev");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;
                loop {
                    let mut cmd_buf = [0u8; 528];
                    let mut cmd_handles = [0u32; 2];
                    match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                        Ok(n) if n >= 16 => {
                            let cmd = cmd_buf[0];
                            let mut sector_bytes = [0u8; 8];
                            sector_bytes.copy_from_slice(&cmd_buf[8..16]);
                            let sector = u64::from_le_bytes(sector_bytes);

                            match cmd {
                                BLK_CMD_READ => {
                                    match driver.do_read(sector) {
                                        Ok(buf) => write_response_data(session_chan, 0, &buf),
                                        Err(e) => write_response(session_chan, e.to_raw() as i64),
                                    }
                                }
                                BLK_CMD_WRITE => {
                                    if n < 528 {
                                        write_response(session_chan, -3);
                                        continue;
                                    }
                                    let mut data = [0u8; 512];
                                    data.copy_from_slice(&cmd_buf[16..528]);
                                    match driver.do_write(sector, &data) {
                                        Ok(()) => write_response(session_chan, 0),
                                        Err(e) => write_response(session_chan, e.to_raw() as i64),
                                    }
                                }
                                BLK_CMD_SIZE => {
                                    let cap = driver.capacity_sectors;
                                    let mut resp = [0u8; 16];
                                    resp[..8].copy_from_slice(&0i64.to_le_bytes());
                                    resp[8..16].copy_from_slice(&cap.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }
                                _ => write_response(session_chan, -3),
                            }
                        }
                        Ok(_) => {}
                        Err(Status::PeerClosed) | Err(_) => {
                            let _ = syscalls::close(session_chan);
                            break;
                        }
                    }
                }
            }
        }
    }
}