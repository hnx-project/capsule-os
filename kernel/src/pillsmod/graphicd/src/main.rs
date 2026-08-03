#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_warn, log_error, syscalls};
use shared::status::{Result, Status};

const QUEUE_SIZE: usize = 16;
const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 960;

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
    pub entry: VirtioGpuMemEntry,
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

unsafe fn submit_command(
    gpu_base: usize,
    desc_table: *mut VirtqDesc,
    avail_ring: *mut u16,
    _used_ring: *mut u16,
    avail_idx: &mut u16,
    q_vmo: usize,    // Pass q_vmo handle for secure EL1 cache flushing
    buf_vmo: usize,  // Pass buf_vmo handle for secure EL1 cache flushing
    cmd_phys: u64,
    cmd_len: usize,
    resp_phys: u64,
    resp_ptr: *mut VirtioGpuRespHdr,
) -> Result<()> {
    // Clear response buffer and safely flush through EL1 privileged syscall
    core::ptr::write_volatile(&mut (*resp_ptr).r#type, 0xffffffff);
    let _ = syscalls::display_flush(buf_vmo);

    // Desc 0: Command (Read-Only)
    *desc_table.add(0) = VirtqDesc {
        addr: cmd_phys,
        len: cmd_len as u32,
        flags: VIRTQ_DESC_F_NEXT,
        next: 1,
    };

    // Desc 1: Response (Write-Only)
    *desc_table.add(1) = VirtqDesc {
        addr: resp_phys,
        len: core::mem::size_of::<VirtioGpuRespHdr>() as u32,
        flags: VIRTQ_DESC_F_WRITE,
        next: 0,
    };

    // Put Desc 0 on available ring
    let ring_idx_offset = (*avail_idx % QUEUE_SIZE as u16) as usize;
    core::ptr::write_volatile(avail_ring.add(2 + ring_idx_offset), 0);
    *avail_idx = avail_idx.wrapping_add(1);
    core::ptr::write_volatile(avail_ring.add(1), *avail_idx);

    // Securely flush modified CPU cache lines for the Command and Virtqueue buffers through EL1 syscalls
    let _ = syscalls::display_flush(buf_vmo);
    let _ = syscalls::display_flush(q_vmo);

    // Notify card
    core::ptr::write_volatile((gpu_base + 0x050) as *mut u32, 0); // QueueNotify = 0

    // Poll wait for response, doing privileged flush in loop to synchronize RAM writes
    loop {
        let _ = syscalls::display_flush(buf_vmo);
        let r_type = core::ptr::read_volatile(&(*resp_ptr).r#type);
        if r_type != 0xffffffff {
            break;
        }
        let _ = syscalls::yield_cpu();
    }

    let resp_type = core::ptr::read_volatile(&(*resp_ptr).r#type);
    if resp_type == 0x1100 { // VIRTIO_GPU_RESP_OK_NODATA
        Ok(())
    } else {
        log_error!("GPUD", "[ERROR] GPU Command failed with response: {:#x}", resp_type);
        Err(Status::InvalidArgs)
    }
}

#[no_mangle]
pub fn main() -> i32 {
    log_info!("GPUD", "User-Space Virtio-GPU UMDF Driver booting...");

    // 1. Probe Virtio-GPU MMIO slots
    let mut gpu_base: usize = 0;
    // Map the entire 16KB Virtio MMIO block starting at 0x0a000000 (which is perfectly page-aligned)
    let vmo_res = syscalls::vmo_create_physical(0x0a000000, 16384);
    if let Ok(vmo) = vmo_res {
        let target_va = 0x5000000;
        let map_res = syscalls::vmar_map_self(vmo, target_va, 16384, 11);
        if map_res.is_ok() {
            for slot in 0..32 {
                let slot_va = target_va + slot * 0x200;
                let magic = unsafe { core::ptr::read_volatile(slot_va as *const u32) };
                let dev_id = unsafe { core::ptr::read_volatile((slot_va + 0x008) as *const u32) };
                if magic == 0x74726976 && dev_id == 16 {
                    log_info!("GPUD", "Discovered Virtio-GPU device at slot {} MMIO {:#x}", slot, 0x0a000000 + slot * 0x200);
                    gpu_base = slot_va;
                    break;
                }
            }
        }
    }

    if gpu_base == 0 {
        log_warn!("GPUD", "No physical Virtio-GPU device found. Exiting gracefully to satisfy DAG dependencies.");
        let _ = libcapsule::notify_init("graphicd");
        loop {
            let _ = syscalls::yield_cpu();
        }
    }

    // 2. Allocate Virtqueue buffers (Using physically contiguous reserved memory immediately following the Framebuffer)
    let q_vmo = syscalls::vmo_create_physical(0x4a4b0000, 8192).unwrap();
    let q_va = 0x6000000;
    syscalls::vmar_map_self(q_vmo, q_va, 8192, 11).unwrap();
    unsafe { core::ptr::write_bytes(q_va as *mut u8, 0, 8192); }

    let desc_table = q_va as *mut VirtqDesc;
    let avail_ring = (q_va + 16 * QUEUE_SIZE) as *mut u16; // q_va + 256
    let used_ring = (q_va + 4096) as *mut u16;            // Page 2 (offset 4096)

    let q_phys = 0x4a4b0000usize;

    // 3. Allocate Dedicated Contiguous Command Request & Response buffers (Using reserved physical memory)
    let buf_vmo = syscalls::vmo_create_physical(0x4a4b2000, 4096).unwrap();
    let buf_va = 0x7000000;
    syscalls::vmar_map_self(buf_vmo, buf_va, 4096, 11).unwrap();
    unsafe { core::ptr::write_bytes(buf_va as *mut u8, 0, 4096); }

    let buf_phys = 0x4a4b2000usize;

    let req_create = buf_va as *mut VirtioGpuResourceCreate2d;
    let req_attach = (buf_va + 256) as *mut VirtioGpuResourceAttachBackingCombined;
    let req_scanout = (buf_va + 768) as *mut VirtioGpuSetScanout;
    let req_transfer = (buf_va + 1024) as *mut VirtioGpuTransferToHost2d;
    let req_flush = (buf_va + 1280) as *mut VirtioGpuResourceFlush;
    let resp_ptr = (buf_va + 2048) as *mut VirtioGpuRespHdr;

    let req_create_phys = buf_phys + 0;
    let req_attach_phys = buf_phys + 256;
    let req_scanout_phys = buf_phys + 768;
    let req_transfer_phys = buf_phys + 1024;
    let req_flush_phys = buf_phys + 1280;
    let resp_phys = buf_phys + 2048;

    let mut avail_idx = 0u16;

    // 4. Connect and Activate the Virtio-GPU Device
    unsafe {
        // A. Reset device
        core::ptr::write_volatile(gpu_base as *mut u32, 0);

        // B. Set status to ACKNOWLEDGE and DRIVER
        core::ptr::write_volatile((gpu_base + 0x070) as *mut u32, 1 | 2);

        // C. Negotiate features (Accept all offered features)
        let f0 = core::ptr::read_volatile((gpu_base + 0x010) as *const u32);
        core::ptr::write_volatile((gpu_base + 0x020) as *mut u32, f0);

        // D. Set FEATURES_OK status bit
        core::ptr::write_volatile((gpu_base + 0x070) as *mut u32, 1 | 2 | 8);

        // E. Set GuestPageSize to 4096 so the device can calculate PFN offsets properly
        core::ptr::write_volatile((gpu_base + 0x028) as *mut u32, 4096);

        // F. Select and configure Virtqueue 0 (Control Queue)
        core::ptr::write_volatile((gpu_base + 0x030) as *mut u32, 0); // QueueSel = 0
        core::ptr::write_volatile((gpu_base + 0x038) as *mut u32, QUEUE_SIZE as u32); // QueueNum
        core::ptr::write_volatile((gpu_base + 0x03c) as *mut u32, 4096); // QueueAlign
        core::ptr::write_volatile((gpu_base + 0x040) as *mut u32, (q_phys / 4096) as u32); // QueuePFN

        // G. Finally set DRIVER_OK status to activate the device pipeline
        core::ptr::write_volatile((gpu_base + 0x070) as *mut u32, 1 | 2 | 8 | 4);
    }

    // 5. Allocate physical continuous framebuffer VMO mapped at 0x4a000000 (size: 4915200, page-aligned)
    let fb_size = 4915200;
    let fb_vmo = syscalls::vmo_create_physical(0x4a000000, fb_size).unwrap();
    let fb_va = 0x8000000;
    syscalls::vmar_map_self(fb_vmo, fb_va, fb_size, 11).unwrap();

    let resource_id = 1u32;

    // 6. Setup Virtio-GPU Resource pipeline
    unsafe {
        // A. CMD_RESOURCE_CREATE_2D
        core::ptr::write_volatile(req_create, VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr {
                r#type: 0x0101, // CREATE_2D
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
        log_info!("GPUD", "Submitting CMD_RESOURCE_CREATE_2D...");
        submit_command(
            gpu_base,
            desc_table,
            avail_ring,
            used_ring,
            &mut avail_idx,
            q_vmo,
            buf_vmo,
            req_create_phys as u64,
            core::mem::size_of::<VirtioGpuResourceCreate2d>(),
            resp_phys as u64,
            resp_ptr,
        ).unwrap();
        log_info!("GPUD", "CMD_RESOURCE_CREATE_2D returned successfully!");

        // B. CMD_RESOURCE_ATTACH_BACKING
        core::ptr::write_volatile(req_attach, VirtioGpuResourceAttachBackingCombined {
            hdr: VirtioGpuCtrlHdr {
                r#type: 0x0106, // ATTACH_BACKING
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                padding: 0,
            },
            resource_id,
            nr_entries: 1,
            entry: VirtioGpuMemEntry {
                addr: 0x4a000000,
                length: fb_size as u32,
                padding: 0,
            },
        });
        submit_command(
            gpu_base,
            desc_table,
            avail_ring,
            used_ring,
            &mut avail_idx,
            q_vmo,
            buf_vmo,
            req_attach_phys as u64,
            core::mem::size_of::<VirtioGpuResourceAttachBackingCombined>(),
            resp_phys as u64,
            resp_ptr,
        ).unwrap();

        // C. CMD_SET_SCANOUT
        core::ptr::write_volatile(req_scanout, VirtioGpuSetScanout {
            hdr: VirtioGpuCtrlHdr {
                r#type: 0x0103, // SET_SCANOUT
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
        submit_command(
            gpu_base,
            desc_table,
            avail_ring,
            used_ring,
            &mut avail_idx,
            q_vmo,
            buf_vmo,
            req_scanout_phys as u64,
            core::mem::size_of::<VirtioGpuSetScanout>(),
            resp_phys as u64,
            resp_ptr,
        ).unwrap();
    }

    log_info!("GPUD", "Hardware initialization complete. Registering IPC service...");

    // 7. Create service channel for client (display-compositor)
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            log_error!("GPUD", "channel_create failed");
            return -2;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;

    if let Err(e) = syscalls::channel_register("svc.gpu", server_chan) {
        log_error!("GPUD", "channel_register failed: {:?}", e);
        return -3;
    }
    log_info!("GPUD", "Registered 'svc.gpu' on channel {}", server_chan);

    let _ = libcapsule::notify_init("graphicd");

    let mut active_session = 0usize;

    loop {
        // A. Poll the service-channel (server_chan) non-blockingly to accept
        //    new compositor connections. When a connection arrives, the first
        //    transferred handle is the compositor's "server-end" of the new
        //    session pair. From that point onward we serve the compositor
        //    exclusively on that handle (it is paired with the compositor's
        //    client-end which it will write to and read from).
        let mut conn_buf = [0u8; 16];
        let mut conn_handles = [0u32; 2];
        let non_block_server = server_chan | 0x80000000;
        if let Ok(_) = syscalls::channel_read(non_block_server, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;
                if active_session == 0 {
                    active_session = session_chan;
                    log_info!("GPUD", "Connected display-compositor session! Sending fb_vmo handle...");

                    // Duplicate fb_vmo and hand it to the compositor on its
                    // session channel. The compositor is blocked on
                    // channel_read(session_chan) waiting for this response.
                    if let Ok(dup_handle) = syscalls::handle_duplicate(fb_vmo, 11) {
                        let mut resp_buf = [0u8; 16];
                        resp_buf[0..4].copy_from_slice(&SCREEN_WIDTH.to_le_bytes());
                        resp_buf[4..8].copy_from_slice(&SCREEN_HEIGHT.to_le_bytes());
                        let _ = syscalls::channel_write(session_chan, &resp_buf, &[dup_handle as u32]);
                        let _ = syscalls::close(dup_handle);
                    }
                }
            }
        }

        // B. Serve the active session. Block-read on its channel; the
        //    compositor alternates between writing commands (0x20=flush) and
        //    reading acknowledgements on this same channel.
        if active_session != 0 {
            let mut cmd_buf = [0u8; 16];
            let mut cmd_handles = [0u32; 2];
            if let Ok(_) = syscalls::channel_read(active_session, &mut cmd_buf, &mut cmd_handles) {
                if cmd_buf[0] == 0x20 {
                    unsafe {
                        // Clean/Flush compositor's written pixels in the
                        // framebuffer backplane from cache to physical RAM.
                        let _ = syscalls::display_flush(fb_vmo);

                        // 1. Transfer RAM Backplane content to host 2D resource
                        core::ptr::write_volatile(req_transfer, VirtioGpuTransferToHost2d {
                            hdr: VirtioGpuCtrlHdr {
                                r#type: 0x0105, // TRANSFER_TO_HOST_2D
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
                        let _ = submit_command(
                            gpu_base,
                            desc_table,
                            avail_ring,
                            used_ring,
                            &mut avail_idx,
                            q_vmo,
                            buf_vmo,
                            req_transfer_phys as u64,
                            core::mem::size_of::<VirtioGpuTransferToHost2d>(),
                            resp_phys as u64,
                            resp_ptr,
                        );

                        // 2. Instruct QEMU SDL window to flush and repaint
                        core::ptr::write_volatile(req_flush, VirtioGpuResourceFlush {
                            hdr: VirtioGpuCtrlHdr {
                                r#type: 0x0104, // RESOURCE_FLUSH
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
                        let _ = submit_command(
                            gpu_base,
                            desc_table,
                            avail_ring,
                            used_ring,
                            &mut avail_idx,
                            q_vmo,
                            buf_vmo,
                            req_flush_phys as u64,
                            core::mem::size_of::<VirtioGpuResourceFlush>(),
                            resp_phys as u64,
                            resp_ptr,
                        );
                    }
                }
            }
        }

        let _ = syscalls::yield_cpu();
    }
}
