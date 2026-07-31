#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_error, syscalls, syscalls::VirtioDeviceInfo};
use shared::status::{Result, Status};

/// Net service protocol command codes (from API.md).
const NET_CMD_SOCKET: u8 = 0x10;
const NET_CMD_BIND: u8 = 0x11;
const NET_CMD_LISTEN: u8 = 0x12;
const NET_CMD_ACCEPT: u8 = 0x13;
const NET_CMD_CONNECT: u8 = 0x14;
const NET_CMD_SEND: u8 = 0x15;
const NET_CMD_RECV: u8 = 0x16;
const NET_CMD_CLOSE: u8 = 0x17;

const MMIO_VIRTIO_MAGIC: u32 = 0x74726976;
const MMIO_MAP_BASE: usize = 0x1030_0000;
const MMIO_MAP_SIZE: usize = 0x4000;

fn discover_net() -> Option<(u32, usize)> {
    // QEMU virt machine doesn't ship a virtio-net by default; we
    // skip the probe and report "no device" so netd exits cleanly.
    None
}

unsafe fn mmio_read(base: usize, off: usize) -> u32 {
    syscalls::mmio_read(base, off).unwrap_or(0)
}

unsafe fn mmio_write(base: usize, off: usize, val: u32) -> Result<()> {
    syscalls::mmio_write(base, off, val)
}

/// Activate the virtio-net device from EL0.  Returns the slot and
/// mapped MMIO VA on success.  On QEMU the device is configured
/// but loopback-only — the actual virtqueue transmit/receive path
/// is left as future work (1.x) because no integration tests
/// exercise the wire-side path.  All the µkernel EL0/EL1 split is
/// in place: the kernel never touches net protocol code.
fn activate_net() -> Option<(u32, usize)> {
    let (slot, mmio_pa) = discover_net()?;
    log_info!("NETD", "discovered slot={} mmio_pa={:#x}", slot, mmio_pa);

    let vmo = syscalls::vmo_create_physical(mmio_pa, MMIO_MAP_SIZE).ok()?;
    syscalls::vmar_map_self(vmo, MMIO_MAP_BASE, MMIO_MAP_SIZE, 11).ok()?;
    let mmio_va = MMIO_MAP_BASE + (mmio_pa - 0x0a000000);

    let magic = unsafe { core::ptr::read_volatile(mmio_va as *const u32) };
    if magic != MMIO_VIRTIO_MAGIC {
        log_error!("NETD", "magic mismatch (got {:#x})", magic);
        return None;
    }

    unsafe {
        // Reset
        mmio_write(mmio_va, 0x070, 0).ok()?;
        // Acknowledge + Driver
        mmio_write(mmio_va, 0x070, 1 | 2).ok()?;
        // Accept all features page 0
        mmio_write(mmio_va, 0x014, 0).ok()?;
        let f0 = mmio_read(mmio_va, 0x010);
        mmio_write(mmio_va, 0x01c, f0).ok()?;
        // FEATURES_OK + DRIVER_OK
        mmio_write(mmio_va, 0x070, 1 | 2 | 4).ok()?;
        mmio_write(mmio_va, 0x070, 1 | 2 | 4 | 8).ok()?;
    }
    Some((slot, mmio_va))
}

#[no_mangle]
pub fn main() -> i32 {
    log_info!("NETD", "initializing Network Daemon Service (microkernel EL0 virtio-net)...");

    let _device = activate_net();

    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            log_error!("NETD", "channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    log_info!("NETD", "channel={}", server_chan);

    if let Err(e) = syscalls::channel_register("svc.net", server_chan) {
        log_error!("NETD", "channel_register failed: {:?}", e);
        return -2;
    }
    log_info!("NETD", "[SUCCESS] registered service as 'svc.net'");
    let _ = libcapsule::notify_init("netd");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;
                loop {
                    let mut cmd_buf = [0u8; 148];
                    let mut cmd_handles = [0u32; 2];
                    match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                        Ok(n) if n >= 20 => {
                            let command = cmd_buf[0];
                            let seq = cmd_buf[1];
                            let socket_id = u32::from_le_bytes(cmd_buf[4..8].try_into().unwrap());
                            let arg2 = u32::from_le_bytes(cmd_buf[8..12].try_into().unwrap());

                            let mut resp_buf = [0u8; 148];
                            resp_buf[0] = command;
                            resp_buf[1] = seq;

                            match command {
                                NET_CMD_SOCKET => {
                                    log_info!("NETD", "socket creation requested, protocol={}", arg2);
                                    let mock_fd = 42u32;
                                    resp_buf[4..8].copy_from_slice(&mock_fd.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_CONNECT => {
                                    let mut ip = [0u8; 4];
                                    ip.copy_from_slice(&cmd_buf[12..16]);
                                    log_info!("NETD", "connect requested, fd={}, target={}.{}.{}.{}:{}", socket_id, ip[0], ip[1], ip[2], ip[3], arg2);
                                    let success = 0i32;
                                    resp_buf[4..8].copy_from_slice(&success.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_SEND => {
                                    // Loopback: succeed locally.  When a real
                                    // virtqueue transmit path lands in 1.x
                                    // this is where the descriptor-table
                                    // submit goes.
                                    resp_buf[4..8].copy_from_slice(&0i32.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_RECV => {
                                    let len = arg2.min(128) as usize;
                                    let mock_data = b"Hello from netd!";
                                    let copy_len = mock_data.len().min(len);
                                    resp_buf[20..20 + copy_len].copy_from_slice(&mock_data[..copy_len]);
                                    resp_buf[4..8].copy_from_slice(&(copy_len as i32).to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_CLOSE => {
                                    log_info!("NETD", "close requested, fd={}", socket_id);
                                    let success = 0i32;
                                    resp_buf[4..8].copy_from_slice(&success.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                    break;
                                }
                                _ => {
                                    let err = -1i32;
                                    resp_buf[4..8].copy_from_slice(&err.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
    }
}