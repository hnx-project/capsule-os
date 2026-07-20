use libcapsule::syscalls;
use libcapsule::Status;

const PROTOCOL_MAGIC: u8 = 0xD1;
const PROTOCOL_VERSION: u8 = 0x01;

const DEV_PROBE: u8 = 0x01;
const DEV_LIST: u8 = 0x02;
const DEV_INFO: u8 = 0x03;
const DEV_OPEN: u8 = 0x04;
const DEV_CLOSE: u8 = 0x05;
const DEV_READ: u8 = 0x06;
const DEV_WRITE: u8 = 0x07;

const REQUEST_HEADER_SIZE: usize = 16;

fn make_request(command: u8, payload: &[u8]) -> [u8; 256] {
    let mut buf = [0u8; 256];
    buf[0] = PROTOCOL_MAGIC;
    buf[1] = PROTOCOL_VERSION;
    buf[2] = command;
    buf[3] = 0;
    let seq_bytes = 1u32.to_le_bytes();
    buf[4..8].copy_from_slice(&seq_bytes);
    let plen = payload.len().min(240);
    if plen > 0 {
        buf[16..16 + plen].copy_from_slice(&payload[..plen]);
    }
    buf
}

fn read_response(buf: &[u8]) -> (i16, &[u8]) {
    if buf.len() < 16 {
        return (-1, &[]);
    }
    let status = i16::from_le_bytes([buf[0], buf[1]]);
    let plen = u32::from_le_bytes(buf[8..12].try_into().unwrap_or([0; 4])) as usize;
    let end = (16 + plen).min(buf.len());
    (status, &buf[16..end])
}

pub fn test_dev_connect() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let _ = syscalls::close(chan);
    true
}

pub fn test_dev_probe() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let req = make_request(DEV_PROBE, &[]);
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (status, _) = read_response(&resp[..n]);
    let _ = syscalls::close(chan);
    status == 0
}

pub fn test_dev_list() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let req = make_request(DEV_LIST, &[]);
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (status, payload) = read_response(&resp[..n]);
    let ok = status == 0 && {
        let s = core::str::from_utf8(payload).unwrap_or("");
        s.contains("pl011")
    };
    let _ = syscalls::close(chan);
    ok
}

pub fn test_dev_info_pl011() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let req = make_request(DEV_INFO, b"0");
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (status, payload) = read_response(&resp[..n]);
    let ok = status == 0 && {
        let s = core::str::from_utf8(payload).unwrap_or("");
        s.contains("pl011") && s.contains("base")
    };
    let _ = syscalls::close(chan);
    ok
}

fn close_chan(chan: usize) {
    let _ = syscalls::close(chan);
}

pub fn test_dev_open_close() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };

    let mut ok = false;

    // Open "pl011"
    let req = make_request(DEV_OPEN, b"pl011");
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        close_chan(chan);
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s,
        Err(_) => {
            close_chan(chan);
            return false;
        }
    };
    let (status, payload) = read_response(&resp[..n]);
    if status != 0 || payload.len() < 4 {
        close_chan(chan);
        return false;
    }
    let handle = u32::from_le_bytes(payload[..4].try_into().unwrap());

    // Close the handle
    let mut nb = [0u8; 12];
    let mut i = 12;
    let mut v = handle as u64;
    while v > 0 && i > 0 {
        i -= 1;
        nb[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if i == 12 {
        nb[11] = b'0';
        i = 11;
    }
    let handle_str = core::str::from_utf8(&nb[i..]).unwrap_or("0");
    let mut close_payload = [0u8; 12];
    let cp_len = handle_str.len().min(close_payload.len());
    close_payload[..cp_len].copy_from_slice(&handle_str.as_bytes()[..cp_len]);
    let req2 = make_request(DEV_CLOSE, &close_payload[..cp_len]);
    if syscalls::channel_write(chan, &req2, &[]).is_err() {
        close_chan(chan);
        return false;
    }
    let n2 = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s,
        Err(_) => {
            close_chan(chan);
            return false;
        }
    };
    let (status2, _) = read_response(&resp[..n2]);
    ok = status2 == 0;

    close_chan(chan);
    ok
}

fn handle_to_str(handle: u32) -> [u8; 12] {
    let mut nb = [0u8; 12];
    let mut i = 12;
    let mut v = handle as u64;
    while v > 0 && i > 0 {
        i -= 1;
        nb[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if i == 12 {
        nb[11] = b'0';
        i = 11;
    }
    let mut out = [0u8; 12];
    let len = 12 - i;
    out[..len].copy_from_slice(&nb[i..]);
    out
}

pub fn test_dev_read_uart() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };

    let req = make_request(DEV_OPEN, b"pl011");
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        close_chan(chan); return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s, Err(_) => { close_chan(chan); return false; }
    };
    let (status, payload) = read_response(&resp[..n]);
    if status != 0 || payload.len() < 4 {
        close_chan(chan); return false;
    }
    let handle = u32::from_le_bytes(payload[..4].try_into().unwrap());
    if handle != 0 {
        close_chan(chan); return false;
    }

    let hstr = handle_to_str(handle);
    let hlen = hstr.iter().position(|&b| b == 0).unwrap_or(12);
    let req2 = make_request(DEV_READ, &hstr[..hlen]);
    let read_payload = b" 24";
    let mut combined = [0u8; 64];
    let cplen = hlen + read_payload.len();
    combined[..hlen].copy_from_slice(&hstr[..hlen]);
    combined[hlen..cplen].copy_from_slice(read_payload);
    let req2 = make_request(DEV_READ, &combined[..cplen]);
    if syscalls::channel_write(chan, &req2, &[]).is_err() {
        close_chan(chan); return false;
    }
    let n2 = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s, Err(_) => { close_chan(chan); return false; }
    };
    let (status2, payload2) = read_response(&resp[..n2]);
    let ok = status2 == 0 && payload2.len() >= 4;

    close_chan(chan);
    ok
}

pub fn test_dev_write_uart() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };

    let req = make_request(DEV_OPEN, b"pl011");
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        close_chan(chan); return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s, Err(_) => { close_chan(chan); return false; }
    };
    let (status, payload) = read_response(&resp[..n]);
    if status != 0 || payload.len() < 4 {
        close_chan(chan); return false;
    }
    let handle = u32::from_le_bytes(payload[..4].try_into().unwrap());

    let hstr = handle_to_str(handle);
    let hlen = hstr.iter().position(|&b| b == 0).unwrap_or(12);
    // Write value 0x00 to DR (offset 0x000) — safe, writes a null byte
    let write_payload = b" 0 0";
    let mut combined = [0u8; 64];
    let cplen = hlen + write_payload.len();
    combined[..hlen].copy_from_slice(&hstr[..hlen]);
    combined[hlen..cplen].copy_from_slice(write_payload);
    let req2 = make_request(DEV_WRITE, &combined[..cplen]);
    if syscalls::channel_write(chan, &req2, &[]).is_err() {
        close_chan(chan); return false;
    }
    let n2 = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s, Err(_) => { close_chan(chan); return false; }
    };
    let (status2, _) = read_response(&resp[..n2]);

    close_chan(chan);
    status2 == 0
}

pub fn test_dev_open_nonexist() -> bool {
    let chan = match syscalls::channel_lookup("svc.dev") {
        Ok(h) => h,
        Err(_) => return false,
    };
    let req = make_request(DEV_OPEN, b"no_such_device");
    if syscalls::channel_write(chan, &req, &[]).is_err() {
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = match syscalls::channel_read(chan, &mut resp, &mut handles) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let (status, _) = read_response(&resp[..n]);
    let _ = syscalls::close(chan);
    status == Status::NotFound.to_raw() as i16
}
