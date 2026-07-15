#![no_std]
#![no_main]

extern crate hnxlibc;

use hnxlibc::syscalls::{self, PROC_MGMT_CREATE, PROC_MGMT_EXIT, PROC_MGMT_RELEASE_PT};

fn print(s: &str) {
    unsafe { hnxlibc::write(1, s.as_ptr(), s.len()); }
}

fn println(s: &str) {
    print(s);
    print("\n");
}

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
    println("procmgr: CapsuleOS process manager starting...");

    let server_chan = match syscalls::channel_create() {
        Ok(ch) => ch,
        Err(_) => {
            println("procmgr: failed to create channel");
            return -1;
        }
    };

    if let Err(_) = syscalls::channel_register("svc.procmgr", server_chan) {
        println("procmgr: failed to register svc.procmgr");
        return -2;
    }

    println("procmgr: registered svc.procmgr, entering event loop");

    loop {
        let mut buf = [0u8; 256];
        let mut handles = [0u32; 4];

        match syscalls::channel_read(server_chan, &mut buf, &mut handles) {
            Ok(n) if n > 0 => {
                let cmd = buf[0];
                match cmd {
                    0 => {
                        if n >= 17 {
                            let ppid = u64::from_le_bytes(buf[1..9].try_into().unwrap_or([0; 8]));
                            let l0_pa = u64::from_le_bytes(buf[9..17].try_into().unwrap_or([0; 8]));
                            let name_len = (n - 17).min(31);
                            let name = core::str::from_utf8(&buf[17..17 + name_len]).unwrap_or("");
                            match syscalls::proc_mgmt(PROC_MGMT_CREATE, ppid as usize, 0, l0_pa as usize) {
                                Ok(pid) => {
                                    add_entry(pid as u64, ppid, l0_pa, name);
                                    if handles.len() > 0 && handles[0] != 0 {
                                        let resp = pid.to_le_bytes();
                                        let _ = syscalls::channel_write(handles[0] as usize, &resp, &[]);
                                    }
                                }
                                Err(e) => {
                                    if handles.len() > 0 && handles[0] != 0 {
                                        let resp = (e.to_raw() as u64).to_le_bytes();
                                        let _ = syscalls::channel_write(handles[0] as usize, &resp, &[]);
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
                    _ => {}
                }
            }
            _ => {
                let _ = syscalls::yield_cpu();
            }
        }
    }
}
