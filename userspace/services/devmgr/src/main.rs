#![no_std]
#![no_main]

extern crate libcapsule;

mod protocol;
mod device;

use libcapsule::{kprintln, syscalls};
use protocol::*;
use device::MAX_OPEN_PER_SESSION;
use shared::status::Status;

fn handle_cmd(session_idx: usize, session_chan: usize, buf: &[u8], _handles: &[u32]) {
    if buf.len() < REQUEST_HEADER_SIZE {
        return;
    }

    let header = match RequestHeader::from_bytes(buf) {
        Some(h) => h,
        None => return,
    };

    let payload = if buf.len() > REQUEST_HEADER_SIZE {
        &buf[REQUEST_HEADER_SIZE..]
    } else {
        &[]
    };

    let seq = header.seq;

    match header.command {
        DEV_PROBE => {
            let mut dev_buf = [0u8; shared::device_info::DEVICE_BUFFER_SIZE];
            let status = match syscalls::device_info(&mut dev_buf) {
                Ok(n) => device::parse_kernel_buffer(&dev_buf[..n]),
                Err(e) => e.to_raw() as i32,
            };
            let resp = build_response(seq, status as i16, &[]);
            let _ = syscalls::channel_write(session_chan, &resp, &[]);
        }

        DEV_LIST => {
            let mut name_buf = [0u8; MAX_PAYLOAD_SIZE];
            let n = device::list_device_names(&mut name_buf);
            let resp = build_response(seq, 0, &name_buf[..n]);
            let _ = syscalls::channel_write(session_chan, &resp, &[]);
        }

        DEV_INFO => {
            let idx_str = core::str::from_utf8(payload).unwrap_or("");
            let idx = parse_usize(idx_str);
            let mut info_buf = [0u8; MAX_PAYLOAD_SIZE];
            match device::encode_device_info(idx, &mut info_buf) {
                Some(n) => {
                    let resp = build_response(seq, 0, &info_buf[..n]);
                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                }
                None => {
                    let resp = build_response(seq, Status::NotFound.to_raw() as i16, &[]);
                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                }
            }
        }

        DEV_OPEN => {
            let raw = core::str::from_utf8(payload).unwrap_or("");
            let name = raw.trim_end_matches('\0');
            match device::find_device(name) {
                Some(dev_idx) => {
                    match device::session_open(session_idx, dev_idx) {
                        Some(handle) => {
                            let mut handle_buf = [0u8; 4];
                            handle_buf.copy_from_slice(&handle.to_le_bytes());
                            let resp = build_response(seq, 0, &handle_buf);
                            let _ = syscalls::channel_write(session_chan, &resp, &[]);
                        }
                        None => {
                            let resp = build_response(seq, Status::NoMemory.to_raw() as i16, &[]);
                            let _ = syscalls::channel_write(session_chan, &resp, &[]);
                        }
                    }
                }
                None => {
                    let resp = build_response(seq, Status::NotFound.to_raw() as i16, &[]);
                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                }
            }
        }

        DEV_CLOSE => {
            let handle_str = core::str::from_utf8(payload).unwrap_or("");
            let handle = parse_u32(handle_str);
            if device::session_close(session_idx, handle) {
                let resp = build_response(seq, 0, &[]);
                let _ = syscalls::channel_write(session_chan, &resp, &[]);
            } else {
                let resp = build_response(seq, Status::InvalidArgs.to_raw() as i16, &[]);
                let _ = syscalls::channel_write(session_chan, &resp, &[]);
            }
        }

        DEV_READ => {
            let s = core::str::from_utf8(payload).unwrap_or("");
            let parts: [usize; 2] = parse_two_usizes(s);
            let (handle, offset) = (parts[0], parts[1]);
            match device::get_device_for_handle(session_idx, handle as u32) {
                Some(entry) => {
                    match syscalls::mmio_read(entry.base as usize, offset) {
                        Ok(val) => {
                            let mut val_buf = [0u8; 4];
                            val_buf.copy_from_slice(&val.to_le_bytes());
                            let resp = build_response(seq, 0, &val_buf);
                            let _ = syscalls::channel_write(session_chan, &resp, &[]);
                        }
                        Err(e) => {
                            let resp = build_response(seq, e.to_raw() as i16, &[]);
                            let _ = syscalls::channel_write(session_chan, &resp, &[]);
                        }
                    }
                }
                None => {
                    let resp = build_response(seq, Status::InvalidArgs.to_raw() as i16, &[]);
                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                }
            }
        }

        DEV_WRITE => {
            let s = core::str::from_utf8(payload).unwrap_or("");
            let parts: [usize; 3] = parse_three_usizes(s);
            let (handle, offset, value) = (parts[0], parts[1], parts[2]);
            match device::get_device_for_handle(session_idx, handle as u32) {
                Some(entry) => {
                    let status = match syscalls::mmio_write(entry.base as usize, offset, value as u32) {
                        Ok(()) => 0i16,
                        Err(e) => e.to_raw() as i16,
                    };
                    let resp = build_response(seq, status, &[]);
                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                }
                None => {
                    let resp = build_response(seq, Status::InvalidArgs.to_raw() as i16, &[]);
                    let _ = syscalls::channel_write(session_chan, &resp, &[]);
                }
            }
        }

        _ => {
            let resp = build_response(seq, Status::InvalidArgs.to_raw() as i16, &[]);
            let _ = syscalls::channel_write(session_chan, &resp, &[]);
        }
    }
}

fn parse_usize(s: &str) -> usize {
    let trimmed = s.trim();
    let mut n: usize = 0;
    for b in trimmed.bytes() {
        if b < b'0' || b > b'9' {
            break;
        }
        n = n.wrapping_mul(10).wrapping_add((b - b'0') as usize);
    }
    n
}

fn parse_u32(s: &str) -> u32 {
    parse_usize(s) as u32
}

fn parse_two_usizes(s: &str) -> [usize; 2] {
    let trimmed = s.trim();
    let mut parts = [0usize; 2];
    let mut pi = 0;
    let mut cur = 0usize;
    let mut in_num = false;
    for b in trimmed.bytes() {
        if b >= b'0' && b <= b'9' {
            cur = cur.wrapping_mul(10).wrapping_add((b - b'0') as usize);
            in_num = true;
        } else if in_num {
            if pi < 2 {
                parts[pi] = cur;
                pi += 1;
            }
            cur = 0;
            in_num = false;
        }
    }
    if in_num && pi < 2 {
        parts[pi] = cur;
    }
    parts
}

fn parse_three_usizes(s: &str) -> [usize; 3] {
    let trimmed = s.trim();
    let mut parts = [0usize; 3];
    let mut pi = 0;
    let mut cur = 0usize;
    let mut in_num = false;
    for b in trimmed.bytes() {
        if b >= b'0' && b <= b'9' {
            cur = cur.wrapping_mul(10).wrapping_add((b - b'0') as usize);
            in_num = true;
        } else if in_num {
            if pi < 3 {
                parts[pi] = cur;
                pi += 1;
            }
            cur = 0;
            in_num = false;
        }
    }
    if in_num && pi < 3 {
        parts[pi] = cur;
    }
    parts
}

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("devmgr: init");

    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            kprintln!("devmgr: channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    kprintln!("devmgr: channel={}", server_chan);

    if let Err(e) = syscalls::channel_register("svc.dev", server_chan) {
        kprintln!("devmgr: channel_register failed: {:?}", e);
        return -2;
    }
    kprintln!("devmgr: registered svc.dev");

    let mut dev_buf = [0u8; shared::device_info::DEVICE_BUFFER_SIZE];
    match syscalls::device_info(&mut dev_buf) {
        Ok(n) => {
            let status = device::parse_kernel_buffer(&dev_buf[..n]);
            if status == 0 {
                let count = unsafe { device::DEVICE_TABLE.count };
                kprintln!("devmgr: {} device(s) discovered", count);
            } else {
                kprintln!("devmgr: parse error: {}", status);
            }
        }
        Err(e) => {
            kprintln!("devmgr: device_info syscall failed: {:?}", e);
        }
    }

    let _ = libcapsule::notify_init("devmgr");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;

                let session_idx = match device::find_free_session() {
                    Some(idx) => {
                        unsafe {
                            device::SESSIONS[idx] = Some(device::Session {
                                handles: [const { None }; MAX_OPEN_PER_SESSION],
                            });
                        }
                        idx
                    }
                    None => {
                        let _ = syscalls::close(session_chan);
                        continue;
                    }
                };

                loop {
                    let mut cmd_buf = [0u8; 256];
                    let mut cmd_handles = [0u32; 2];
                    match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                        Ok(n) if n > 0 => {
                            handle_cmd(session_idx, session_chan, &cmd_buf[..n], &cmd_handles);
                        }
                        Ok(_) => {}
                        Err(Status::PeerClosed) | Err(_) => {
                            let _ = syscalls::close(session_chan);
                            device::session_cleanup(session_idx);
                            break;
                        }
                    }
                }
            }
        }
    }
}
