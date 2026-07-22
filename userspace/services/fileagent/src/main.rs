#![no_std]
#![no_main]

extern crate capsule_runtime;
extern crate libcapsule;

mod protocol;
mod ramfs;
mod vfs;

use libcapsule::{kprintln, syscalls};
use protocol::*;
use shared::status::Status;

fn cmd_path(buf: &[u8]) -> &str {
    if buf.len() <= 20 {
        return "";
    }
    let end = buf[20..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(buf.len() - 20);
    core::str::from_utf8(&buf[20..20 + end]).unwrap_or("")
}

fn cmd_path_old(buf: &[u8]) -> &str {
    if buf.len() <= 20 {
        return "";
    }
    let slice = if buf.len() > 84 { &buf[20..84] } else { &buf[20..] };
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    core::str::from_utf8(&slice[..end]).unwrap_or("")
}

fn cmd_path_new(buf: &[u8]) -> &str {
    if buf.len() <= 84 {
        return "";
    }
    let slice = if buf.len() > 148 { &buf[84..148] } else { &buf[84..] };
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    core::str::from_utf8(&slice[..end]).unwrap_or("")
}

fn cmd_arg32_1(buf: &[u8]) -> u32 {
    if buf.len() < 8 {
        return 0;
    }
    u32::from_le_bytes(buf[4..8].try_into().unwrap_or([0; 4]))
}

fn cmd_arg32_2(buf: &[u8]) -> u32 {
    if buf.len() < 12 {
        return 0;
    }
    u32::from_le_bytes(buf[8..12].try_into().unwrap_or([0; 4]))
}

fn cmd_arg64(buf: &[u8]) -> u64 {
    if buf.len() < 20 {
        return 0;
    }
    u64::from_le_bytes(buf[12..20].try_into().unwrap_or([0; 8]))
}

fn write_response(channel: usize, val: i64) {
    let resp = val.to_le_bytes();
    let _ = syscalls::channel_write(channel, &resp, &[]);
}

fn write_response_data(channel: usize, val: i64, data: &[u8]) {
    let mut resp = [0u8; 8 + 128];
    resp[..8].copy_from_slice(&val.to_le_bytes());
    let copy_len = data.len().min(128);
    resp[8..8 + copy_len].copy_from_slice(&data[..copy_len]);
    let _ = syscalls::channel_write(channel, &resp[..8 + copy_len], &[]);
}

fn handle_cmd(session_idx: usize, session_chan: usize, buf: &[u8], _handles: &[u32]) {
    if buf.is_empty() {
        return;
    }
    let cmd = buf[0];

    match cmd {
        VFS_OPEN => {
            let path = cmd_path(buf);
            let flags = cmd_arg32_1(buf);
            let result = vfs::do_open(session_idx, path, flags);
            write_response(session_chan, result as i64);
        }

        VFS_CLOSE => {
            let fd = cmd_arg32_1(buf);
            let result = vfs::do_close(session_idx, fd);
            write_response(session_chan, result as i64);
        }

        VFS_READ => {
            let fd = cmd_arg32_1(buf);
            let mut data = [0u8; 1024];
            let result = vfs::do_read(session_idx, fd, &mut data);
            if result >= 0 {
                write_response_data(session_chan, result as i64, &data[..result as usize]);
            } else {
                write_response(session_chan, result as i64);
            }
        }

        VFS_WRITE => {
            let fd = cmd_arg32_1(buf);
            let len = cmd_arg32_2(buf) as usize;
            let data = if buf.len() > 20 && len > 0 {
                &buf[20..20 + len]
            } else if buf.len() > 20 {
                &buf[20..]
            } else {
                &[]
            };
            let result = vfs::do_write(session_idx, fd, data);
            write_response(session_chan, result as i64);
        }

        VFS_MKDIR => {
            let path = cmd_path(buf);
            let result = vfs::do_mkdir(path);
            write_response(session_chan, result as i64);
        }

        VFS_RMDIR => {
            let path = cmd_path(buf);
            let result = vfs::do_rmdir(path);
            write_response(session_chan, result as i64);
        }

        VFS_UNLINK => {
            let path = cmd_path(buf);
            let result = vfs::do_unlink(path);
            write_response(session_chan, result as i64);
        }

        VFS_READDIR => {
            let fd = cmd_arg32_1(buf);
            let mut data = [0u8; 1024];
            let result = vfs::do_readdir(session_idx, fd, &mut data);
            if result >= 0 {
                write_response_data(session_chan, result as i64, &data[..result as usize]);
            } else {
                write_response(session_chan, result as i64);
            }
        }

        VFS_STAT => {
            let path = cmd_path(buf);
            let (size, ntype) = vfs::do_stat(path);
            let mut resp = [0u8; 16];
            resp[..8].copy_from_slice(&(size as i64).to_le_bytes());
            resp[8..16].copy_from_slice(&(ntype as i64).to_le_bytes());
            let _ = syscalls::channel_write(session_chan, &resp, &[]);
        }

        VFS_RENAME => {
            let old_path = cmd_path_old(buf);
            let new_path = cmd_path_new(buf);
            let result = vfs::do_rename(old_path, new_path);
            write_response(session_chan, result as i64);
        }

        _ => {
            write_response(session_chan, Status::InvalidArgs.to_raw() as i64);
        }
    }
}

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("fileagent: init");

    ramfs::init();

    if ramfs::mkdir_path("/dev") < 0 {
        kprintln!("fileagent: mkdir /dev failed");
        return -1;
    }

    kprintln!("fileagent: creating channel");

    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            kprintln!("fileagent: channel_create failed");
            return -1;
        }
    };
    let server_chan = (raw >> 32) as u32 as usize;
    kprintln!("fileagent: channel_create ok, server_chan={}", server_chan);

    if let Err(e) = syscalls::channel_register("svc.vfs", server_chan) {
        kprintln!("fileagent: channel_register failed: {:?}", e);
        return -2;
    }
    kprintln!("fileagent: registered svc.vfs");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        // 1. Block-wait for a new connection (0% CPU, no busy loops)
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;

                // Track the session in the available slot
                let sessions = vfs::sessions_mut();
                let mut session_idx = None;
                for i in 0..sessions.len() {
                    if sessions[i].is_none() {
                        sessions[i] = Some(vfs::Session {
                            server_chan: session_chan,
                            fds: [const { None }; 16],
                            devmgr_chan: 0,
                        });
                        session_idx = Some(i);
                        break;
                    }
                }

                if let Some(idx) = session_idx {
                    // 2. Drive the active session sequentially until closed
                    loop {
                        let mut cmd_buf = [0u8; 148];
                        let mut cmd_handles = [0u32; 2];
                        match syscalls::channel_read(session_chan, &mut cmd_buf, &mut cmd_handles) {
                            Ok(n) => {
                                if n > 0 {
                                    handle_cmd(idx, session_chan, &cmd_buf[..n], &cmd_handles);
                                }
                            }
                            Err(Status::PeerClosed) | Err(_) => {
                                let _ = syscalls::close(session_chan);
                                let sessions = vfs::sessions_mut();
                                if let Some(ref sess) = sessions[idx] {
                                    if sess.devmgr_chan != 0 {
                                        let _ = syscalls::close(sess.devmgr_chan);
                                    }
                                }
                                sessions[idx] = None;
                                break;
                            }
                        }
                    }
                } else {
                    let _ = syscalls::close(session_chan);
                }
            }
        }
    }
}
