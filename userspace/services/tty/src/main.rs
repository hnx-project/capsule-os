#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{kprint, kprintln, syscalls};
use shared::status::Status;

/// TTY service protocol command codes.
const TTY_CMD_READ: u8 = 1;
const TTY_CMD_WRITE: u8 = 2;
const TTY_CMD_IOCTL: u8 = 3;
const TTY_CMD_BIND_PGID: u8 = 4;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("tty: initializing Terminal & TTY Console Service...");

    // 1. Create a bidirectional server channel
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            kprintln!("tty: [ERROR] channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    kprintln!("tty: server channel handle={}", server_chan);

    // 2. Register global name "svc.tty"
    if let Err(e) = syscalls::channel_register("svc.tty", server_chan) {
        kprintln!("tty: [ERROR] channel_register failed: {:?}", e);
        return -2;
    }
    kprintln!("tty: [SUCCESS] registered service as 'svc.tty'");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    // 3. TTY service main event loop (non-polling, blocking)
    loop {
        conn_buf.fill(0);
        conn_handles.fill(0);

        // Blocking wait for connection requests
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;
                kprintln!("tty: accepted incoming connection session={}", session_chan);

                // Handle session commands until the connection closes
                loop {
                    let mut cmd_buf = [0u8; 148]; // 148-byte fixed aligned packet
                    let mut cmd_handles = [0u32; 2];

                    match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                        Ok(n) if n >= 1 => {
                            let command = cmd_buf[0];
                            let seq = cmd_buf[1];

                            match command {
                                TTY_CMD_READ => {
                                    let mut len_bytes = [0u8; 4];
                                    len_bytes.copy_from_slice(&cmd_buf[4..8]);
                                    let max_len = (u32::from_le_bytes(len_bytes) as usize).min(128);

                                    let mut read_buf = [0u8; 128];
                                     // Direct read from kernel UART (blocking canonical read)
                                     let read_res = libcapsule::syscall!(
                                        shared::syscall_nums::SYSCALL_READ,
                                        0, // fd = 0
                                        read_buf.as_mut_ptr() as usize,
                                        max_len,
                                        0,
                                        0,
                                        0
                                    ) as isize;

                                    let actual_len = if read_res >= 0 { read_res as usize } else { 0 };

                                    let mut resp = [0u8; 148];
                                    resp[0] = TTY_CMD_READ;
                                    resp[1] = seq;
                                    resp[4..8].copy_from_slice(&(actual_len as u32).to_le_bytes());
                                    if actual_len > 0 {
                                        resp[20..20 + actual_len].copy_from_slice(&read_buf[..actual_len]);
                                    }
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }

                                TTY_CMD_WRITE => {
                                    let mut len_bytes = [0u8; 4];
                                    len_bytes.copy_from_slice(&cmd_buf[4..8]);
                                    let len = u32::from_le_bytes(len_bytes) as usize;
                                    let write_len = len.min(128);

                                    if write_len > 0 {
                                         // Direct physical write bypass
                                         let _ = libcapsule::syscall!(
                                            shared::syscall_nums::SYSCALL_WRITE,
                                            1, // fd = 1
                                            cmd_buf[20..].as_ptr() as usize,
                                            write_len,
                                            0,
                                            0,
                                            0
                                        );
                                    }

                                    let mut resp = [0u8; 148];
                                    resp[0] = TTY_CMD_WRITE;
                                    resp[1] = seq;
                                    resp[4..8].copy_from_slice(&(write_len as u32).to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }

                                TTY_CMD_IOCTL => {
                                    let mut resp = [0u8; 148];
                                    resp[0] = TTY_CMD_IOCTL;
                                    resp[1] = seq;
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }

                                TTY_CMD_BIND_PGID => {
                                    let mut resp = [0u8; 148];
                                    resp[0] = TTY_CMD_BIND_PGID;
                                    resp[1] = seq;
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }

                                _ => {
                                    let mut resp = [0u8; 148];
                                    resp[0] = command;
                                    resp[1] = seq;
                                    resp[4..8].copy_from_slice(&(Status::InvalidArgs.to_raw() as i32).to_le_bytes());
                                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(Status::PeerClosed) | Err(_) => {
                            kprintln!("tty: session closed, closing channel handle {}", session_chan);
                            let _ = syscalls::close(session_chan);
                            break;
                        }
                    }
                }
            }
        }
    }
}
