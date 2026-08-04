use libcapsule::syscalls;

pub fn test_proc_connect() -> bool {
    match syscalls::channel_lookup("svc.procmgr") {
        Ok(chan) => {
            let _ = syscalls::close(chan);
            true
        }
        Err(_) => false,
    }
}

pub fn test_proc_create_invalid() -> bool {
    let procmgr_chan = match syscalls::channel_lookup("svc.procmgr") {
        Ok(h) => h,
        Err(_) => return false,
    };

    // Create dual endpoints local channel pair
    let raw = match syscalls::channel_create() {
        Ok(v) => v,
        Err(_) => {
            let _ = syscalls::close(procmgr_chan);
            return false;
        }
    };
    let client_end = raw as u32 as usize;
    let server_end = (raw >> 32) as u32 as usize;

    // Construct a Command 0 (Create) request:
    // buf[0] = 0 (Create)
    // buf[1..9] = ppid (1)
    // buf[9..17] = l0_pa (0, invalid)
    // buf[17..] = name ("dummy")
    let mut buf = [0u8; 64];
    buf[0] = 0;
    buf[1..9].copy_from_slice(&1u64.to_le_bytes());
    buf[9..17].copy_from_slice(&0u64.to_le_bytes());
    let name = b"dummy";
    buf[17..17 + name.len()].copy_from_slice(name);

    // Write request to procmgr transferring control of server_end
    let handles = [server_end as u32];
    if syscalls::channel_write(procmgr_chan, &buf[..17 + name.len()], &handles).is_err() {
        let _ = syscalls::close(procmgr_chan);
        let _ = syscalls::close(client_end);
        let _ = syscalls::close(server_end);
        return false;
    }

    // Wait for procmgr status report on client_end
    let mut resp = [0u8; 8];
    let mut resp_handles = [0u32; 2];
    let n = match syscalls::channel_read(client_end, &mut resp, &mut resp_handles) {
        Ok(read_bytes) => read_bytes,
        Err(_) => {
            let _ = syscalls::close(procmgr_chan);
            let _ = syscalls::close(client_end);
            return false;
        }
    };

    let _ = syscalls::close(procmgr_chan);
    let _ = syscalls::close(client_end);

    if n < 8 {
        return false;
    }

    // Invalid parameters must trigger a negative return status from kernel / procmgr
    let status_val = i64::from_le_bytes(resp);
    status_val < 0
}
