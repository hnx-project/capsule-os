#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_warn, log_error, syscalls};
use shared::status::{Result, Status};

const QUEUE_SIZE: usize = 64;

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
pub struct MouseStatePacket {
    pub x: i32,
    pub y: i32,
    pub down: bool,
}

#[no_mangle]
pub fn main() -> i32 {
    log_info!("INPUTD", "User-Space Virtio-Input UMDF Driver booting...");

    // 1. Probe Virtio-Input MMIO slots
    let mut input_base: usize = 0;
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
                if magic == 0x74726976 && dev_id == 18 {
                    // Per virtio 1.3 §5.8 + §4.2.2:
                    //   - cfg_select at base + 0x100 (8-bit write)
                    //   - cfg_subsel at base + 0x101 (8-bit write)
                    //   - cfg_size   at base + 0x102 (8-bit read)
                    //   - payload    at base + 0x108+ (8-bit reads)
                    //   - status     at base + 0x070 (32-bit)
                    // Spec §4.2.2.2 requires 8-bit MMIO accesses for 8-bit fields.
                    //
                    // We probe VIRTIO_INPUT_CFG_EV_BITS (select=0x11) twice,
                    // once with subsel=EV_REL (0x02) and once with subsel=EV_ABS
                    // (0x03). A non-zero size field tells us the device supports
                    // that event type. Mouse => EV_REL only. Touch => EV_ABS only.
                    unsafe {
                        let status_before = core::ptr::read_volatile((slot_va + 0x070) as *const u32);
                        if (status_before & 1) == 0 {
                            core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 1);
                        }
                        // Probe EV_REL sub-capability (which REL_* codes exist)
                        core::ptr::write_volatile((slot_va + 0x100) as *mut u8, 0x11); // CFG_EV_BITS
                        core::ptr::write_volatile((slot_va + 0x101) as *mut u8, 0x02); // EV_REL
                        let size_rel = core::ptr::read_volatile((slot_va + 0x102) as *const u8);
                        let r0 = core::ptr::read_volatile((slot_va + 0x108) as *const u8);
                        let r1 = core::ptr::read_volatile((slot_va + 0x109) as *const u8);
                        let r2 = core::ptr::read_volatile((slot_va + 0x10a) as *const u8);
                        let r3 = core::ptr::read_volatile((slot_va + 0x10b) as *const u8);
                        let ev_rel_size = size_rel;
                        // Probe EV_ABS sub-capability (which ABS_* codes exist)
                        core::ptr::write_volatile((slot_va + 0x100) as *mut u8, 0x11); // CFG_EV_BITS
                        core::ptr::write_volatile((slot_va + 0x101) as *mut u8, 0x03); // EV_ABS
                        let size_abs = core::ptr::read_volatile((slot_va + 0x102) as *const u8);
                        let a0 = core::ptr::read_volatile((slot_va + 0x108) as *const u8);
                        let a1 = core::ptr::read_volatile((slot_va + 0x109) as *const u8);
                        let a2 = core::ptr::read_volatile((slot_va + 0x10a) as *const u8);
                        let a3 = core::ptr::read_volatile((slot_va + 0x10b) as *const u8);
                        let ev_abs_size = size_abs;

                        let slot_owned = (status_before & 1) != 0;

                        log_info!(
                            "INPUTD",
                            "Slot {} probe: size_rel={} (rel_bytes={:02x}{:02x}{:02x}{:02x}), size_abs={} (abs_bytes={:02x}{:02x}{:02x}{:02x})",
                            slot, ev_rel_size, r0, r1, r2, r3, ev_abs_size, a0, a1, a2, a3
                        );

                        // Pure mouse: EV_REL has non-zero size, EV_ABS is zero.
                        // Mixed (tablet with REL+ABS in cfg) is left for touchd
                        // because a mixed slot is more likely a QEMU tablet
                        // that emits ABS_MT_* events, which only touchd parses.
                        if (ev_rel_size > 0) && (ev_abs_size == 0) {
                            log_info!(
                                "INPUTD",
                                "Discovered Virtio-Input pure-mouse at slot {} MMIO {:#x}",
                                slot, 0x0a000000 + slot * 0x200
                            );
                            input_base = slot_va;
                            break;
                        } else if (ev_abs_size > 0) && (ev_rel_size == 0) {
                            log_info!(
                                "INPUTD",
                                "Slot {} is a touch-only sub-device; leaving for touchd",
                                slot
                            );
                            if !slot_owned {
                                core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 0);
                            }
                            continue;
                        } else if (ev_rel_size > 0) && (ev_abs_size > 0) {
                            // Mixed slot — likely a QEMU tablet reporting both
                            // event types. Leave it for touchd to handle the
                            // multi-touch stream; inputd's REL-only parser
                            // would silently miss every ABS_MT_* event.
                            log_info!(
                                "INPUTD",
                                "Slot {} is a mixed (REL+ABS) sub-device; leaving for touchd",
                                slot
                            );
                            if !slot_owned {
                                core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 0);
                            }
                            continue;
                        } else {
                            log_warn!(
                                "INPUTD",
                                "Slot {} has unrecognized input sub-type (size_rel={}, size_abs={}); skipping",
                                slot, ev_rel_size, ev_abs_size
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
        // Fallback: precise cfg probe failed to find a mouse. Try to claim
        // the first dev_id==18 slot that nobody else has ACKNOWLEDGEd.
        // This protects us against QEMU versions where the cfg bitmap
        // differs from what the device actually emits on the eventq
        // (e.g. a virtio-mouse-device that doesn't advertise EV_REL in cfg
        // but still emits relative mouse events).
        log_warn!(
            "INPUTD",
            "No mouse found via cfg probe; entering fallback scan"
        );
        for slot in 0..32usize {
            let slot_va = 0x5000000usize + slot * 0x200;
            let magic = unsafe { core::ptr::read_volatile(slot_va as *const u32) };
            let dev_id = unsafe { core::ptr::read_volatile((slot_va + 0x008) as *const u32) };
            if magic != 0x74726976 || dev_id != 18 {
                continue;
            }
            unsafe {
                let status = core::ptr::read_volatile((slot_va + 0x070) as *const u32);
                if (status & 1) != 0 {
                    // Owned by touchd or another driver — skip without reset.
                    continue;
                }
                core::ptr::write_volatile((slot_va + 0x070) as *mut u32, 1);
                log_warn!(
                    "INPUTD",
                    "Fallback: claiming slot {} MMIO {:#x} as mouse (cfg probe failed)",
                    slot, 0x0a000000 + slot * 0x200
                );
                input_base = slot_va;
                break;
            }
        }
    }

    if input_base == 0 {
        log_warn!("INPUTD", "No physical Virtio-Input device found. Exiting gracefully to satisfy DAG dependencies.");
        // Notify servicesd that we are "ready" to avoid blocking downstream services
        let _ = libcapsule::notify_init("inputd");
        loop {
            let _ = syscalls::yield_cpu();
        }
    }

    // 2. Allocate Virtqueue buffers (Requires 2 pages to align Used Ring at 4096-byte boundary)
    let q_vmo = syscalls::vmo_create(8192).unwrap();
    let q_va = 0x6000000;
    syscalls::vmar_map_self(q_vmo, q_va, 8192, 11).unwrap();
    unsafe { core::ptr::write_bytes(q_va as *mut u8, 0, 8192); }

    let desc_table = q_va as *mut VirtqDesc;
    let avail_ring = (q_va + 16 * QUEUE_SIZE) as *mut u16; // q_va + 1024 (Immediately following Descriptor Table)
    let used_ring = (q_va + 4096) as *mut u16;            // Page 2 (offset 4096, strictly aligned to QueueAlign)

    let q_phys = syscalls::vmo_get_phys(q_vmo, 0).unwrap();

    // 3. Allocate Event Buffers
    let b_vmo = syscalls::vmo_create(4096).unwrap();
    let b_va = 0x7000000;
    syscalls::vmar_map_self(b_vmo, b_va, 4096, 11).unwrap();
    unsafe { core::ptr::write_bytes(b_va as *mut u8, 0, 4096); }

    let event_buffers = b_va as *mut VirtioInputEvent;
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
        // QueueSel = 0
        core::ptr::write_volatile((input_base + 0x030) as *mut u32, 0);
        // QueueNum = QUEUE_SIZE
        core::ptr::write_volatile((input_base + 0x038) as *mut u32, QUEUE_SIZE as u32);
        // QueueAlign = 4096
        core::ptr::write_volatile((input_base + 0x03c) as *mut u32, 4096);
        // QueuePFN = q_phys / 4096
        core::ptr::write_volatile((input_base + 0x040) as *mut u32, (q_phys / 4096) as u32);

        // Status = ACKNOWLEDGE | DRIVER | FEATURES_OK | DRIVER_OK
        core::ptr::write_volatile((input_base + 0x070) as *mut u32, 1 | 2 | 8 | 4);
        // QueueNotify = 0
        core::ptr::write_volatile((input_base + 0x050) as *mut u32, 0);
    }

    log_info!("INPUTD", "Hardware initialization complete. Registering IPC service...");

    // 6. Create service channel for clients (compositor)
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            log_error!("INPUTD", "channel_create failed");
            return -2;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;

    if let Err(e) = syscalls::channel_register("svc.input", server_chan) {
        log_error!("INPUTD", "channel_register failed: {:?}", e);
        return -3;
    }
    log_info!("INPUTD", "Registered 'svc.input' on channel {}", server_chan);

    let _ = libcapsule::notify_init("inputd");

    // Track active client sessions (compositor)
    let mut active_sessions = [0usize; 8];
    let mut session_count = 0;

    let mut current_mouse = MouseStatePacket { x: 200, y: 150, down: false };
    let mut used_idx = 0u16;

    // Adaptive bounds (default 1280x960, dynamically negotiated on connection)
    let mut screen_w = 1280;
    let mut screen_h = 960;

    // Heartbeat counter: every N idle polling cycles we re-broadcast the
    // current mouse state so the compositor can recover from a lost release
    // event (e.g. if QEMU coalesced up events into a single down-only burst).
    let mut heartbeat_counter: u32 = 0;
    const HEARTBEAT_PERIOD: u32 = 200; // ~200 yields ≈ 200ms

    loop {
        // A. Listen for new client connections (non-blocking style)
        let mut conn_buf = [0u8; 16];
        let mut conn_handles = [0u32; 2];
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 && session_count < 8 {
                let client_chan = conn_handles[0] as usize;
                active_sessions[session_count] = client_chan;
                session_count += 1;
                log_info!("INPUTD", "Connected new client compositor session");

                // Read screen resolution handshake from compositor (using non-blocking channel read)
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
                             log_info!("INPUTD", "Received screen resolution handshake: {}x{}", screen_w, screen_h);
                        }
                        break;
                    }
                    let _ = syscalls::yield_cpu();
                    attempts += 1;
                }
            }
        }

        // B. Poll hardware Virtqueue for mouse events
        unsafe {
            let used_ptr = used_ring as *const VirtqUsed;
            let latest_used_idx = core::ptr::read_volatile(&(*used_ptr).idx);

            if used_idx != latest_used_idx {
                let count = latest_used_idx.wrapping_sub(used_idx) as usize;
                let mut mouse_changed = false;

                for k in 0..count {
                    let ring_slot = (used_idx.wrapping_add(k as u16) as usize) % QUEUE_SIZE;
                    let desc_idx = core::ptr::read_volatile(&(*used_ptr).ring[ring_slot].id) as usize;
                    let ev = core::ptr::read_volatile(event_buffers.add(desc_idx));

                    match ev.r#type {
                        1 => { // EV_KEY
                            if ev.code == 272 { // BTN_LEFT
                                current_mouse.down = ev.value != 0;
                                mouse_changed = true;
                            }
                        }
                        2 => { // EV_REL (Relative)
                            if ev.code == 0 { // REL_X
                                current_mouse.x = (current_mouse.x + ev.value as i32).clamp(0, screen_w - 1);
                                mouse_changed = true;
                            } else if ev.code == 1 { // REL_Y
                                current_mouse.y = (current_mouse.y + ev.value as i32).clamp(0, screen_h - 1);
                                mouse_changed = true;
                            }
                        }
                        3 => { // EV_ABS (Absolute)
                            // inputd only owns pure-mouse slots. EV_ABS
                            // events from those devices use codes 0/1 (ABS_X/Y).
                            if ev.code == 0 { // ABS_X
                                current_mouse.x = ((ev.value as i32 * screen_w) / 32768).clamp(0, screen_w - 1);
                                mouse_changed = true;
                            } else if ev.code == 1 { // ABS_Y
                                current_mouse.y = ((ev.value as i32 * screen_h) / 32768).clamp(0, screen_h - 1);
                                mouse_changed = true;
                            }
                        }
                        _ => {}
                    }

                    // Recycle descriptor back to avail ring safely (no-overwrite Virtqueue Recycle)
                    *avail_ring.add(2 + (avail_idx as usize % QUEUE_SIZE)) = desc_idx as u16;
                    avail_idx = avail_idx.wrapping_add(1);
                }

                used_idx = latest_used_idx;

                // Sync recycling index to avail ring
                *avail_ring.add(1) = avail_idx;
                core::ptr::write_volatile((input_base + 0x050) as *mut u32, 0); // QueueNotify

                // C. Dispatch latest mouse state package to all connected client sessions (e.g. compositor)
                if mouse_changed && session_count > 0 {
                    let mut packet_buf = [0u8; 12];
                    packet_buf[0..4].copy_from_slice(&current_mouse.x.to_le_bytes());
                    packet_buf[4..8].copy_from_slice(&current_mouse.y.to_le_bytes());
                    packet_buf[8] = if current_mouse.down { 1 } else { 0 };

                    let mut i = 0;
                    while i < session_count {
                        let client_chan = active_sessions[i];
                        if let Err(Status::PeerClosed) = syscalls::channel_write(client_chan, &packet_buf, &[]) {
                            // Peer disconnected: clean up session
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

        // Heartbeat: re-broadcast the current mouse state periodically so the
        // compositor can recover from a missed release event. This guards
        // against a stuck-down drag if QEMU coalesced the up event into a
        // burst that our polling missed.
        heartbeat_counter = heartbeat_counter.wrapping_add(1);
        if heartbeat_counter >= HEARTBEAT_PERIOD && session_count > 0 {
            heartbeat_counter = 0;
            let mut packet_buf = [0u8; 12];
            packet_buf[0..4].copy_from_slice(&current_mouse.x.to_le_bytes());
            packet_buf[4..8].copy_from_slice(&current_mouse.y.to_le_bytes());
            packet_buf[8] = if current_mouse.down { 1 } else { 0 };

            let mut i = 0;
            while i < session_count {
                let client_chan = active_sessions[i];
                if let Err(Status::PeerClosed) = syscalls::channel_write(client_chan, &packet_buf, &[]) {
                    let _ = syscalls::close(client_chan);
                    active_sessions[i] = active_sessions[session_count - 1];
                    session_count -= 1;
                } else {
                    i += 1;
                }
            }
        }

        // Tiny delay or voluntary yield to keep CPU usage low
        let _ = syscalls::yield_cpu();
    }
}
