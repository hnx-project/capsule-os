use libcapsule::syscalls;

const NET_CMD_SOCKET: u8 = 0x10;
const NET_CMD_BIND: u8 = 0x11;
const NET_CMD_LISTEN: u8 = 0x12;
const NET_CMD_ACCEPT: u8 = 0x13;
const NET_CMD_CONNECT: u8 = 0x14;
const NET_CMD_SEND: u8 = 0x15;
const NET_CMD_RECV: u8 = 0x16;
const NET_CMD_CLOSE: u8 = 0x17;

pub fn test_net_connect() -> bool {
    match syscalls::channel_lookup("svc.net") {
        Ok(chan) => {
            let _ = syscalls::close(chan);
            true
        }
        Err(_) => false,
    }
}

pub fn test_net_socket_ops() -> bool {
    // 1. Resolve and connect to netd (creates a direct session channel)
    let net_chan = match syscalls::channel_lookup("svc.net") {
        Ok(h) => h,
        Err(_) => return false,
    };

    // 2. Send NET_CMD_SOCKET to allocate socket
    let mut cmd_buf = [0u8; 148];
    cmd_buf[0] = NET_CMD_SOCKET;
    cmd_buf[1] = 1; // seq
    cmd_buf[8..12].copy_from_slice(&1u32.to_le_bytes()); // TCP = 1

    if syscalls::channel_write(net_chan, &cmd_buf, &[]).is_err() {
        let _ = syscalls::close(net_chan);
        return false;
    }

    let mut resp_buf = [0u8; 148];
    let mut resp_handles = [0u32; 2];
    match syscalls::channel_read(net_chan, &mut resp_buf, &mut resp_handles) {
        Ok(n) if n >= 20 => {
            let fd = u32::from_le_bytes(resp_buf[4..8].try_into().unwrap());
            if fd != 42 {
                libcapsule::kprintln!("testall-net: Expected socket fd 42, got {}", fd);
                let _ = syscalls::close(net_chan);
                return false;
            }
        }
        _ => {
            let _ = syscalls::close(net_chan);
            return false;
        }
    }

    // 3. Send NET_CMD_CONNECT
    cmd_buf[0] = NET_CMD_CONNECT;
    cmd_buf[1] = 2; // seq
    cmd_buf[4..8].copy_from_slice(&42u32.to_le_bytes()); // socket_id = 42
    cmd_buf[8..12].copy_from_slice(&80u32.to_le_bytes()); // port = 80
    cmd_buf[12..16].copy_from_slice(&[127, 0, 0, 1]); // IP = 127.0.0.1

    if syscalls::channel_write(net_chan, &cmd_buf, &[]).is_err() {
        let _ = syscalls::close(net_chan);
        return false;
    }

    match syscalls::channel_read(net_chan, &mut resp_buf, &mut resp_handles) {
        Ok(n) if n >= 20 => {
            let status = i32::from_le_bytes(resp_buf[4..8].try_into().unwrap());
            if status != 0 {
                libcapsule::kprintln!("testall-net: Connect failed, status {}", status);
                let _ = syscalls::close(net_chan);
                return false;
            }
        }
        _ => {
            let _ = syscalls::close(net_chan);
            return false;
        }
    }

    // 4. Send NET_CMD_SEND
    cmd_buf[0] = NET_CMD_SEND;
    cmd_buf[1] = 3; // seq
    cmd_buf[8..12].copy_from_slice(&6u32.to_le_bytes()); // length = 6
    cmd_buf[20..26].copy_from_slice(b"Hello!");

    if syscalls::channel_write(net_chan, &cmd_buf, &[]).is_err() {
        let _ = syscalls::close(net_chan);
        return false;
    }

    match syscalls::channel_read(net_chan, &mut resp_buf, &mut resp_handles) {
        Ok(n) if n >= 20 => {
            let status = i32::from_le_bytes(resp_buf[4..8].try_into().unwrap());
            if status != 0 {
                libcapsule::kprintln!("testall-net: Send failed, status {}", status);
                let _ = syscalls::close(net_chan);
                return false;
            }
        }
        _ => {
            let _ = syscalls::close(net_chan);
            return false;
        }
    }

    // 5. Send NET_CMD_RECV
    cmd_buf[0] = NET_CMD_RECV;
    cmd_buf[1] = 4; // seq
    cmd_buf[8..12].copy_from_slice(&16u32.to_le_bytes()); // max_len = 16

    if syscalls::channel_write(net_chan, &cmd_buf, &[]).is_err() {
        let _ = syscalls::close(net_chan);
        return false;
    }

    match syscalls::channel_read(net_chan, &mut resp_buf, &mut resp_handles) {
        Ok(n) if n >= 20 => {
            let read_len = i32::from_le_bytes(resp_buf[4..8].try_into().unwrap());
            if read_len <= 0 {
                libcapsule::kprintln!("testall-net: Recv failed, read_len {}", read_len);
                let _ = syscalls::close(net_chan);
                return false;
            }
            let data = &resp_buf[20..20 + read_len as usize];
            if data != b"Hello from netd!" {
                libcapsule::kprintln!("testall-net: Expected 'Hello from netd!', got '{:?}'", data);
                let _ = syscalls::close(net_chan);
                return false;
            }
        }
        _ => {
            let _ = syscalls::close(net_chan);
            return false;
        }
    }

    // 6. Send NET_CMD_CLOSE
    cmd_buf[0] = NET_CMD_CLOSE;
    cmd_buf[1] = 5; // seq

    if syscalls::channel_write(net_chan, &cmd_buf, &[]).is_err() {
        let _ = syscalls::close(net_chan);
        return false;
    }

    match syscalls::channel_read(net_chan, &mut resp_buf, &mut resp_handles) {
        Ok(n) if n >= 20 => {
            let status = i32::from_le_bytes(resp_buf[4..8].try_into().unwrap());
            if status != 0 {
                libcapsule::kprintln!("testall-net: Close failed, status {}", status);
                let _ = syscalls::close(net_chan);
                return false;
            }
        }
        _ => {
            let _ = syscalls::close(net_chan);
            return false;
        }
    }

    let _ = syscalls::close(net_chan);
    true
}
