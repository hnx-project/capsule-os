#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{kprintln, syscalls};
use libcapsule::syscalls::{PROC_MGMT_CREATE, PROC_MGMT_EXIT, PROC_MGMT_RELEASE_PT};
use shared::status::Status;

#[repr(C)]
struct ProcEntry {
    pid: u64,
    ppid: u64,
    state: u32,
    l0_pa: u64,
    name: [u8; 32],
}

const PROC_TABLE_SIZE: usize = 64;
static mut PROC_TABLE: [Option<ProcEntry>; PROC_TABLE_SIZE] = [const { None }; PROC_TABLE_SIZE];

fn add_entry(pid: u64, ppid: u64, l0_pa: u64, name: &str) {
    unsafe {
        for slot in PROC_TABLE.iter_mut() {
            if slot.is_none() {
                let mut entry = ProcEntry {
                    pid,
                    ppid,
                    state: 1,
                    l0_pa,
                    name: [0u8; 32],
                };
                let name_len = name.len().min(31);
                entry.name[..name_len].copy_from_slice(&name.as_bytes()[..name_len]);
                *slot = Some(entry);
                break;
            }
        }
    }
}

fn mark_zombie(pid: u64) {
    unsafe {
        for slot in PROC_TABLE.iter_mut() {
            if let Some(ref mut p) = slot {
                if p.pid == pid {
                    p.state = 2;
                    break;
                }
            }
        }
    }
}

fn remove_entry(pid: u64) {
    unsafe {
        for slot in PROC_TABLE.iter_mut() {
            if let Some(ref p) = slot {
                if p.pid == pid {
                    *slot = None;
                    break;
                }
            }
        }
    }
}

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("procmgr: CapsuleOS process manager starting...");

    let server_chan = match syscalls::channel_create() {
        Ok(ch) => ch,
        Err(_) => {
            kprintln!("procmgr: failed to create channel");
            return -1;
        }
    };

    if let Err(_) = syscalls::channel_register("svc.procmgr", server_chan) {
        kprintln!("procmgr: failed to register svc.procmgr");
        return -2;
    }

    kprintln!("procmgr: registered svc.procmgr, entering event loop");
    let _ = libcapsule::notify_init("procmgr");

    let mut conn_buf = [0u8; 64];
    let mut conn_handles = [0u32; 2];

    loop {
        if let Ok(_) = syscalls::channel_read(server_chan, &mut conn_buf, &mut conn_handles) {
            if conn_handles[0] != 0 {
                let session_chan = conn_handles[0] as usize;

                loop {
                    let mut buf = [0u8; 256];
                    let mut handles = [0u32; 4];
                    match syscalls::channel_read(session_chan, &mut buf, &mut handles) {
                        Ok(n) if n > 0 => {
                            let cmd = buf[0];
                            match cmd {
                                0 => {
                                    if n >= 17 {
                                        let ppid = u64::from_le_bytes(buf[1..9].try_into().unwrap_or([0; 8]));
                                        let l0_pa = u64::from_le_bytes(buf[9..17].try_into().unwrap_or([0; 8]));
                                        let name_len = (n - 17).min(31);
                                        let name = core::str::from_utf8(&buf[17..17 + name_len]).unwrap_or("");
                                        match syscalls::proc_mgmt(
                                            PROC_MGMT_CREATE,
                                            ppid as usize,
                                            0,
                                            l0_pa as usize,
                                        ) {
                                            Ok(pid) => {
                                                add_entry(pid as u64, ppid, l0_pa, name);
                                                if handles.len() > 0 && handles[0] != 0 {
                                                    let resp = pid.to_le_bytes();
                                                    let _ = syscalls::channel_write(
                                                        handles[0] as usize,
                                                        &resp,
                                                        &[],
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                if handles.len() > 0 && handles[0] != 0 {
                                                    let resp = (e.to_raw() as u64).to_le_bytes();
                                                    let _ = syscalls::channel_write(
                                                        handles[0] as usize,
                                                        &resp,
                                                        &[],
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                1 => {
                                    if n >= 9 {
                                        let pid = u64::from_le_bytes(buf[1..9].try_into().unwrap_or([0; 8]));
                                        mark_zombie(pid);
                                        let _ = syscalls::proc_mgmt(PROC_MGMT_EXIT, pid as usize, 0, 0);
                                    }
                                }
                                 2 => {
                                     if n >= 17 {
                                         let pid = u64::from_le_bytes(buf[1..9].try_into().unwrap_or([0; 8]));
                                         let l0_pa = u64::from_le_bytes(buf[9..17].try_into().unwrap_or([0; 8]));
                                         remove_entry(pid);
                                         let _ = syscalls::proc_mgmt(PROC_MGMT_RELEASE_PT, l0_pa as usize, 0, 0);
                                     }
                                 }
                                 3 => {
                                     // PROC_SNAPSHOT
                                     if handles.len() > 0 && handles[0] != 0 {
                                         let mut resp_buf = [0u8; 4096];
                                         let mut count = 0u32;
                                         let mut offset = 4;
                                         unsafe {
                                             for slot in PROC_TABLE.iter() {
                                                 if let Some(ref entry) = slot {
                                                     if offset + 52 <= resp_buf.len() {
                                                         resp_buf[offset..offset+8].copy_from_slice(&entry.pid.to_le_bytes());
                                                         resp_buf[offset+8..offset+16].copy_from_slice(&entry.ppid.to_le_bytes());
                                                         resp_buf[offset+16..offset+20].copy_from_slice(&entry.state.to_le_bytes());
                                                         resp_buf[offset+20..offset+52].copy_from_slice(&entry.name);
                                                         offset += 52;
                                                         count += 1;
                                                     }
                                                 }
                                             }
                                         }
                                         resp_buf[0..4].copy_from_slice(&count.to_le_bytes());
                                         let _ = syscalls::channel_write(
                                             handles[0] as usize,
                                             &resp_buf[..offset],
                                             &[],
                                         );
                                     }
                                 }
                                _ => {}
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
