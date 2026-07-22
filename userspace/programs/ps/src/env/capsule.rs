use super::{ProcSystem, ProcessInfo};

extern crate libc;
extern crate libcapsule;
extern crate libstd;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn get_process_list(&self, infos: &mut [ProcessInfo]) -> Result<usize, ()> {
        // 1. Look up svc.procmgr
        let session_chan = match libcapsule::syscalls::channel_lookup("svc.procmgr") {
            Ok(ch) => ch,
            Err(_) => return Err(()),
        };

        // 2. Create response channel
        let resp_chan = match libcapsule::syscalls::channel_create() {
            Ok(ch) => ch,
            Err(_) => {
                let _ = libcapsule::syscalls::close(session_chan);
                return Err(());
            }
        };

        // 3. Send command 3 (PROC_SNAPSHOT) with response channel handle
        let mut cmd = [0u8; 1];
        cmd[0] = 3; // Command 3
        if let Err(_) = libcapsule::syscalls::channel_write(session_chan, &cmd, &[resp_chan as u32]) {
            let _ = libcapsule::syscalls::close(session_chan);
            let _ = libcapsule::syscalls::close(resp_chan);
            return Err(());
        }

        // 4. Read response from response channel
        let mut resp_buf = [0u8; 4096];
        let mut handles = [0u32; 2];
        let n = match libcapsule::syscalls::channel_read(resp_chan, &mut resp_buf, &mut handles) {
            Ok(len) if len >= 4 => len,
            _ => {
                let _ = libcapsule::syscalls::close(session_chan);
                let _ = libcapsule::syscalls::close(resp_chan);
                return Err(());
            }
        };

        let _ = libcapsule::syscalls::close(session_chan);
        let _ = libcapsule::syscalls::close(resp_chan);

        // 5. Parse response
        let count = u32::from_le_bytes(resp_buf[0..4].try_into().unwrap_or([0; 4])) as usize;
        let mut offset = 4;
        let limit = count.min(infos.len());
        for i in 0..limit {
            if offset + 52 > n {
                break;
            }
            let pid = u64::from_le_bytes(resp_buf[offset..offset+8].try_into().unwrap_or([0; 8])) as u32;
            let ppid = u64::from_le_bytes(resp_buf[offset+8..offset+16].try_into().unwrap_or([0; 8])) as u32;
            let state = u32::from_le_bytes(resp_buf[offset+16..offset+20].try_into().unwrap_or([0; 4])) as u8;
            
            let mut name = [0u8; 16];
            name.copy_from_slice(&resp_buf[offset+20..offset+36]);
            
            let mut name_len = 0;
            while name_len < 16 && name[name_len] != 0 {
                name_len += 1;
            }

            infos[i] = ProcessInfo {
                pid,
                ppid,
                state,
                name,
                name_len,
            };
            offset += 52;
        }

        Ok(limit)
    }

    fn write_stdout(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn write_stderr(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn exit(&self, code: i32) -> ! {
        libc::exit(code);
    }
}
