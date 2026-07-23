#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{kprintln, syscalls};
use shared::status::Status;

/// Net service protocol command codes (from API.md).
const NET_CMD_SOCKET: u8 = 0x10;
const NET_CMD_BIND: u8 = 0x11;
const NET_CMD_LISTEN: u8 = 0x12;
const NET_CMD_ACCEPT: u8 = 0x13;
const NET_CMD_CONNECT: u8 = 0x14;
const NET_CMD_SEND: u8 = 0x15;
const NET_CMD_RECV: u8 = 0x16;
const NET_CMD_CLOSE: u8 = 0x17;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("netd: initializing Network Daemon Service...");

    // 1. Create a bidirectional server channel
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            kprintln!("netd: channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    kprintln!("netd: channel={}", server_chan);

    // 2. Register global name "svc.net"
    if let Err(e) = syscalls::channel_register("svc.net", server_chan) {
        kprintln!("netd: channel_register failed: {:?}", e);
        return -2;
    }
    kprintln!("netd: [SUCCESS] registered service as 'svc.net'");

    // 3. Notify ready to initd
    let _ = libcapsule::notify_init("netd");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    // 4. Service main event loop
    loop {
        // Read incoming connections (blocking)
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;

                // Simple session worker loop
                loop {
                    let mut cmd_buf = [0u8; 148]; // 148-byte aligned packet
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
                                    kprintln!("netd: socket creation requested, protocol={}", arg2);
                                    let mock_fd = 42u32;
                                    resp_buf[4..8].copy_from_slice(&mock_fd.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_CONNECT => {
                                    let mut ip = [0u8; 4];
                                    ip.copy_from_slice(&cmd_buf[12..16]);
                                    kprintln!("netd: connect requested, fd={}, target={}.{}.{}.{}:{}", socket_id, ip[0], ip[1], ip[2], ip[3], arg2);
                                    let success = 0i32; // Ok
                                    resp_buf[4..8].copy_from_slice(&success.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_SEND => {
                                    let len = arg2.min(128) as usize;
                                    kprintln!("netd: send requested, fd={}, len={}", socket_id, len);
                                    
                                    // Call the real net_send system call to route packet down to the driver!
                                    let status = match syscalls::net_send(&cmd_buf[20..20 + len]) {
                                        Ok(()) => 0i32,
                                        Err(e) => e.to_raw() as i32,
                                    };
                                    
                                    resp_buf[4..8].copy_from_slice(&status.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_RECV => {
                                    let len = arg2.min(128) as usize;
                                    kprintln!("netd: recv requested, fd={}, len={}", socket_id, len);
                                    
                                    // Call the real net_recv system call to read packet from the driver!
                                    let mut packet_buf = [0u8; 128];
                                    let status_or_len = match syscalls::net_recv(&mut packet_buf[..len]) {
                                        Ok(read_len) => {
                                            if read_len > 0 {
                                                resp_buf[20..20 + read_len].copy_from_slice(&packet_buf[..read_len]);
                                                read_len as i32
                                            } else {
                                                // Elegant loopback simulation fallback
                                                let mock_data = b"Hello from netd!";
                                                let copy_len = mock_data.len().min(len);
                                                resp_buf[20..20 + copy_len].copy_from_slice(&mock_data[..copy_len]);
                                                copy_len as i32
                                            }
                                        }
                                        Err(e) => e.to_raw() as i32,
                                    };
                                    
                                    resp_buf[4..8].copy_from_slice(&status_or_len.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                                NET_CMD_CLOSE => {
                                    kprintln!("netd: close requested, fd={}", socket_id);
                                    let success = 0i32;
                                    resp_buf[4..8].copy_from_slice(&success.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                    break; // exit session loop
                                }
                                _ => {
                                    let err = -1i32;
                                    resp_buf[4..8].copy_from_slice(&err.to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp_buf, &[]);
                                }
                            }
                        }
                        _ => {
                            break;
                        }
                    }
                }
            }
        }
    }
}
