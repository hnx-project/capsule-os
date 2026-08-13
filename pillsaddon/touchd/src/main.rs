#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_warn, log_error, syscalls};
use shared::status::Status;

const QUEUE_SIZE: usize = 64;
const MAX_TOUCH_SLOTS: usize = 10;
const TOUCH_VA_BASE: usize = 0x8000000;
const Q_VA: usize = 0x8100000;
const BUF_VA: usize = 0x8200000;

#[repr(C, align(16))]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

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
    pub r#type: u16,
    pub code: u16,
    pub value: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TouchSlot {
    pub tracking_id: i32,
    pub x: i32,
    pub y: i32,
    pub pressure: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TouchStatePacket {
    pub slots: [TouchSlot; MAX_TOUCH_SLOTS],
    pub active_count: u8,
    pub reserved: [u8; 7],
}

const _: () = {
    let packet_size = core::mem::size_of::<TouchStatePacket>();
    let max_payload = 1024;
    assert!(packet_size <= max_payload, "TouchStatePacket must fit channel payload");
};

const _: () = {
    let touch_slot_size = core::mem::size_of::<TouchSlot>();
    assert!(touch_slot_size == 16, "TouchSlot must be 16 bytes (16-byte ABI alignment)");
};

#[no_mangle]
pub fn main() -> i32 {
    log_info!("TOUCHD", "User-Space Virtio-Input Touch Driver booting...");

    // 1. Probe Virtio-Input MMIO slots
    let mut input_base: usize = 0;
    // Map the entire 16KB Virtio MMIO block at 0x0a000000 (page-aligned)
    let vmo_res = syscalls::vmo_create_physical(0x0a000000, 16384);
    if let Ok(vmo) = vmo_res {
        let map_res = syscalls::vmar_map_self(vmo, TOUCH_VA_BASE, 16384, 11);
        if map_res.is_ok() {
            for slot in 0..32 {
                let slot_va = TOUCH_VA_BASE + slot * 0x200;
                let magic = unsafe { core::ptr::read_volatile(slot_va as *const u32) };
                let dev_id = unsafe { core::ptr::read_volatile((slot_va + 0x008) as *const u32) };
                if magic == 0x74726976 && dev_id == 18 {
                    // Per virtio 1.3 §5.8 + §4.2.2:
                    //   - cfg_select at base + 0x100 (8-bit write)
                    //   - cfg_subsel at base + 0x101 (8-bit write)
                    //   - cfg_size   at base + 0x102 (8-bit read)
                    //   - payload    at base + 0x108+ (8-bit reads)
                    //   - status     at base + 0x070 (32-bit)
                    // Spec §4.2.2.2 requires 8-bit MMIO accesses for 8-bit fields.
                    //
                    // Coordination rule: we never reset (write status=0) a
                    // slot that another driver already owns — that would tear
                    // down their initialization (per spec 3.1.1 §2.1.1 the
                    // driver MUST NOT clear a status bit). If a slot is
                    // already ACKNOWLEDGEd by someone else and it's not the
                    // kind we want, we skip it and continue scanning.
                    unsafe {
                        let status_before = core::ptr::read_volatile((slot_va + 0x070) as *const u32);
                        if (status_before & 1) == 0 {
                            core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 1);
                        } else {
                            log_info!(
                                "TOUCHD",
                                "Slot {} already ACKNOWLEDGEd by another driver (status={:#x}); probing read-only",
                                slot, status_before
                            );
                        }
                        // Probe EV_REL sub-capability
                        core::ptr::write_volatile((slot_va + 0x100) as *mut u8, 0x11); // CFG_EV_BITS
                        core::ptr::write_volatile((slot_va + 0x101) as *mut u8, 0x02); // EV_REL
                        let size_rel = core::ptr::read_volatile((slot_va + 0x102) as *const u8);
                        let _r0 = core::ptr::read_volatile((slot_va + 0x108) as *const u8);
                        let _r1 = core::ptr::read_volatile((slot_va + 0x109) as *const u8);
                        let _r2 = core::ptr::read_volatile((slot_va + 0x10a) as *const u8);
                        let _r3 = core::ptr::read_volatile((slot_va + 0x10b) as *const u8);
                        // Probe EV_ABS sub-capability
                        core::ptr::write_volatile((slot_va + 0x100) as *mut u8, 0x11); // CFG_EV_BITS
                        core::ptr::write_volatile((slot_va + 0x101) as *mut u8, 0x03); // EV_ABS
                        let size_abs = core::ptr::read_volatile((slot_va + 0x102) as *const u8);
                        let _a0 = core::ptr::read_volatile((slot_va + 0x108) as *const u8);
                        let _a1 = core::ptr::read_volatile((slot_va + 0x109) as *const u8);
                        let _a2 = core::ptr::read_volatile((slot_va + 0x10a) as *const u8);
                        let _a3 = core::ptr::read_volatile((slot_va + 0x10b) as *const u8);

                        let slot_owned = (status_before & 1) != 0;

                        if (size_abs > 0) && (size_rel == 0) {
                            log_info!(
                                "TOUCHD",
                                "Discovered Virtio-Input pure-touch device at slot {} MMIO {:#x}",
                                slot, 0x0a000000 + slot * 0x200
                            );
                            input_base = slot_va;
                            break;
                        } else if (size_rel > 0) && (size_abs > 0) {
                            // Mixed sub-device (tablet that also reports REL).
                            // inputd explicitly leaves these for us.
                            log_info!(
                                "TOUCHD",
                                "Discovered Virtio-Input mixed (touch+rel) device at slot {} MMIO {:#x}",
                                slot, 0x0a000000 + slot * 0x200
                            );
                            input_base = slot_va;
                            break;
                        } else if (size_rel > 0) && (size_abs == 0) {
                            log_info!(
                                "TOUCHD",
                                "Slot {} is a pure-mouse sub-device (size_rel={}); leaving for inputd",
                                slot, size_rel
                            );
                            if !slot_owned {
                                core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 0);
                            }
                            continue;
                        } else {
                            log_warn!(
                                "TOUCHD",
                                "Slot {} has unrecognized input sub-type (size_rel={}, size_abs={}); skipping",
                                slot, size_rel, size_abs
                            );
                            if !slot_owned {
                                core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 0);
                            }
                            continue;
                        }
                    }
                }
            }
        }
    }

    if input_base == 0 {
        log_warn!("TOUCHD", "No touch-capable Virtio-Input device found. Exiting gracefully.");
        let _ = libcapsule::notify_init("touchd");
        loop {
            let _ = syscalls::yield_cpu();
        }
    }

    // 2. Allocate Virtqueue buffers (Requires 2 pages to align Used Ring at 4096-byte boundary)
    let q_vmo = syscalls::vmo_create(8192).unwrap();
    syscalls::vmar_map_self(q_vmo, Q_VA, 8192, 11).unwrap();
    unsafe { core::ptr::write_bytes(Q_VA as *mut u8, 0, 8192); }

    let desc_table = Q_VA as *mut VirtqDesc;
    let avail_ring = (Q_VA + 16 * QUEUE_SIZE) as *mut u16;
    let used_ring = (Q_VA + 4096) as *mut u16;

    let q_phys = syscalls::vmo_get_phys(q_vmo, 0).unwrap();

    // 3. Allocate Event Buffers
    let b_vmo = syscalls::vmo_create(4096).unwrap();
    syscalls::vmar_map_self(b_vmo, BUF_VA, 4096, 11).unwrap();
    unsafe { core::ptr::write_bytes(BUF_VA as *mut u8, 0, 4096); }

    let event_buffers = BUF_VA as *mut VirtioInputEvent;
    let b_phys = syscalls::vmo_get_phys(b_vmo, 0).unwrap();

    // 4. Initialize Virtqueues
    for i in 0..QUEUE_SIZE {
        let buf_phys = b_phys + i * core::mem::size_of::<VirtioInputEvent>();
        unsafe {
            *desc_table.add(i) = VirtqDesc {
                addr: buf_phys as u64,
                len: core::mem::size_of::<VirtioInputEvent>() as u32,
                flags: VIRTQ_DESC_F_WRITE,
                next: 0,
            };
            *avail_ring.add(2 + i) = i as u16;
        }
    }
    unsafe {
        *avail_ring.add(0) = 0;
        *avail_ring.add(1) = QUEUE_SIZE as u16;
    }
    let mut avail_idx = QUEUE_SIZE as u16;

    // 5. Connect and activate the Virtio Device
    unsafe {
        core::ptr::write_volatile((input_base + 0x030) as *mut u32, 0); // QueueSel = 0
        core::ptr::write_volatile((input_base + 0x038) as *mut u32, QUEUE_SIZE as u32);
        core::ptr::write_volatile((input_base + 0x03c) as *mut u32, 4096);
        core::ptr::write_volatile((input_base + 0x040) as *mut u32, (q_phys / 4096) as u32);
        core::ptr::write_volatile((input_base + 0x070) as *mut u32, 1 | 2 | 8 | 4); // ACK|DRIVER|FEATURES_OK|DRIVER_OK
        core::ptr::write_volatile((input_base + 0x050) as *mut u32, 0); // QueueNotify
    }

    log_info!("TOUCHD", "Hardware initialization complete. Registering IPC service...");

    // 6. Create service channel for clients (compositor)
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            log_error!("TOUCHD", "channel_create failed");
            return -2;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;

    if let Err(e) = syscalls::channel_register("svc.touch", server_chan) {
        log_error!("TOUCHD", "channel_register failed: {:?}", e);
        return -3;
    }
    log_info!("TOUCHD", "Registered 'svc.touch' on channel {}", server_chan);

    let _ = libcapsule::notify_init("touchd");

    let mut active_sessions = [0usize; 8];
    let mut session_count = 0;

    // Linux input-event codes used here (subset):
    //   EV_SYN = 0x00,  SYN_REPORT = 0
    //   EV_KEY = 0x01,  BTN_TOUCH = 0x14a (330)
    //   EV_ABS = 0x03,
    //     ABS_MT_SLOT          = 0x2f  (47)
    //     ABS_MT_POSITION_X    = 0x35  (53)
    //     ABS_MT_POSITION_Y    = 0x36  (54)
    //     ABS_MT_TRACKING_ID   = 0x39  (57)
    //     ABS_MT_PRESSURE      = 0x3a  (58)

    let mut current_touch = TouchStatePacket {
        slots: [TouchSlot {
            tracking_id: -1,
            x: 0,
            y: 0,
            pressure: 0,
        }; MAX_TOUCH_SLOTS],
        active_count: 0,
        reserved: [0u8; 7],
    };
    let mut used_idx = 0u16;

    let mut screen_w = 1280;
    let mut screen_h = 960;

    // Internal parser state — MT Protocol B slot tracking
    let mut current_slot: usize = 0;

    loop {
        // A. Listen for new client connections
        let mut conn_buf = [0u8; 16];
        let mut conn_handles = [0u32; 2];
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 && session_count < 8 {
                let client_chan = conn_handles[0] as usize;
                active_sessions[session_count] = client_chan;
                session_count += 1;
                log_info!("TOUCHD", "Connected new client compositor session");

                let mut res_buf = [0u8; 16];
                let mut res_handles = [0u32; 2];
                let mut attempts = 0;
                while attempts < 100 {
                    if let Ok(_) = syscalls::channel_read(client_chan | 0x80000000, &mut res_buf, &mut res_handles) {
                        let w = u32::from_le_bytes(res_buf[0..4].try_into().unwrap()) as i32;
                        let h = u32::from_le_bytes(res_buf[4..8].try_into().unwrap()) as i32;
                        if w > 0 && h > 0 {
                            screen_w = w;
                            screen_h = h;
                            log_info!("TOUCHD", "Received screen resolution handshake: {}x{}", screen_w, screen_h);
                        }
                        break;
                    }
                    let _ = syscalls::yield_cpu();
                    attempts += 1;
                }
            }
        }

        // B. Poll hardware Virtqueue for touch events
        unsafe {
            let used_ptr = used_ring as *const VirtqUsed;
            let latest_used_idx = core::ptr::read_volatile(&(*used_ptr).idx);

            if used_idx != latest_used_idx {
                let count = latest_used_idx.wrapping_sub(used_idx) as usize;
                let mut touch_changed = false;

                for k in 0..count {
                    let ring_slot = (used_idx.wrapping_add(k as u16) as usize) % QUEUE_SIZE;
                    let desc_idx = core::ptr::read_volatile(&(*used_ptr).ring[ring_slot].id) as usize;
                    let ev = core::ptr::read_volatile(event_buffers.add(desc_idx));

                    match ev.r#type {
                        0 => {
                            // EV_SYN
                            if ev.code == 0 {
                                // SYN_REPORT: count active slots and dispatch
                                current_touch.active_count = 0;
                                for slot_idx in 0..MAX_TOUCH_SLOTS {
                                    if current_touch.slots[slot_idx].tracking_id >= 0 {
                                        current_touch.active_count += 1;
                                    }
                                }
                                touch_changed = true;
                            }
                        }
                        1 => {
                            // EV_KEY
                            if ev.code == 330 {
                                // BTN_TOUCH — global touch engaged flag.
                                // We do not gate on it; multi-touch already conveys
                                // engagement via tracking_id >= 0.
                            }
                        }
                        3 => {
                            // EV_ABS — MT Protocol B
                            match ev.code {
                                47 => {
                                    // ABS_MT_SLOT
                                    current_slot = (ev.value as usize) % MAX_TOUCH_SLOTS;
                                }
                                53 => {
                                    // ABS_MT_POSITION_X
                                    current_touch.slots[current_slot].x =
                                        ((ev.value as i32 * screen_w) / 32768).clamp(0, screen_w - 1);
                                }
                                54 => {
                                    // ABS_MT_POSITION_Y
                                    current_touch.slots[current_slot].y =
                                        ((ev.value as i32 * screen_h) / 32768).clamp(0, screen_h - 1);
                                }
                                57 => {
                                    // ABS_MT_TRACKING_ID — release if 0xffffffff, otherwise set
                                    let raw = ev.value;
                                    if raw == 0xffffffff {
                                        current_touch.slots[current_slot] = TouchSlot {
                                            tracking_id: -1,
                                            x: 0,
                                            y: 0,
                                            pressure: 0,
                                        };
                                    } else {
                                        current_touch.slots[current_slot].tracking_id = raw as i32;
                                    }
                                }
                                58 => {
                                    // ABS_MT_PRESSURE
                                    current_touch.slots[current_slot].pressure = ev.value;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }

                    // Recycle descriptor back to avail ring
                    *avail_ring.add(2 + (avail_idx as usize % QUEUE_SIZE)) = desc_idx as u16;
                    avail_idx = avail_idx.wrapping_add(1);
                }

                used_idx = latest_used_idx;
                *avail_ring.add(1) = avail_idx;
                core::ptr::write_volatile((input_base + 0x050) as *mut u32, 0); // QueueNotify

                // C. Dispatch latest touch state to all connected sessions
                if touch_changed && session_count > 0 {
                    let packet_bytes = core::slice::from_raw_parts(
                        &current_touch as *const _ as *const u8,
                        core::mem::size_of::<TouchStatePacket>(),
                    );

                    let mut i = 0;
                    while i < session_count {
                        let client_chan = active_sessions[i];
                        if let Err(Status::PeerClosed) = syscalls::channel_write(client_chan, packet_bytes, &[]) {
                            let _ = syscalls::close(client_chan);
                            active_sessions[i] = active_sessions[session_count - 1];
                            session_count -= 1;
                        } else {
                            i += 1;
                        }
                    }
                }
            }
        }

        let _ = syscalls::yield_cpu();
    }
}