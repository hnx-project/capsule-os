use crate::ramfs;
use libcapsule::syscalls;
use shared::status::Status;

const MAX_SESSIONS: usize = 8;
const MAX_FDS: usize = 16;

const DEV_MAGIC: u8 = 0xD1;
const DEV_VER: u8 = 0x01;
const DEV_OPEN: u8 = 0x04;
const DEV_CLOSE: u8 = 0x05;
const DEV_READ: u8 = 0x06;
const DEV_WRITE: u8 = 0x07;

pub struct OpenFile {
    pub node_idx: i16,
    pub offset: u32,
    pub dev_handle: u32,
    pub devmgr_chan: usize,
}

pub struct Session {
    pub server_chan: usize,
    pub fds: [Option<OpenFile>; MAX_FDS],
    pub devmgr_chan: usize,
}

static mut SESSIONS: [Option<Session>; MAX_SESSIONS] = [const { None }; MAX_SESSIONS];

pub fn sessions_mut() -> &'static mut [Option<Session>; MAX_SESSIONS] {
    unsafe { &mut SESSIONS }
}

pub const fn tty_fd() -> u32 {
    100
}

fn dev_name_from_path(path: &str) -> Option<&str> {
    let stripped = if path.starts_with("/dev/") {
        &path[5..]
    } else if path.starts_with("dev/") {
        &path[4..]
    } else {
        return None;
    };
    if stripped.is_empty() { None } else { Some(stripped) }
}

fn devmgr_connect() -> Option<usize> {
    syscalls::channel_lookup("svc.dev").ok()
}

fn devmgr_open(devmgr: usize, name: &str) -> Option<u32> {
    let name_bytes = name.as_bytes();
    let plen = name_bytes.len().min(240);
    let mut req = [0u8; 256];
    req[0] = DEV_MAGIC;
    req[1] = DEV_VER;
    req[2] = DEV_OPEN;
    req[4..8].copy_from_slice(&1u32.to_le_bytes());
    req[16..16 + plen].copy_from_slice(&name_bytes[..plen]);

    if syscalls::channel_write(devmgr, &req[..16 + plen], &[]).is_err() {
        return None;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = syscalls::channel_read(devmgr, &mut resp, &mut handles).ok()?;
    if n < 16 {
        return None;
    }
    let status = i16::from_le_bytes([resp[0], resp[1]]);
    if status != 0 {
        return None;
    }
    let plen = u32::from_le_bytes(resp[8..12].try_into().unwrap_or([0; 4])) as usize;
    if plen < 4 {
        return None;
    }
    Some(u32::from_le_bytes(resp[16..20].try_into().unwrap()))
}

fn devmgr_close(devmgr: usize, handle: u32) -> bool {
    let handle_str = {
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
    };
    let hlen = handle_str.iter().position(|&b| b == 0).unwrap_or(12);

    let mut req = [0u8; 256];
    req[0] = DEV_MAGIC;
    req[1] = DEV_VER;
    req[2] = DEV_CLOSE;
    req[4..8].copy_from_slice(&1u32.to_le_bytes());
    req[16..16 + hlen].copy_from_slice(&handle_str[..hlen]);

    if syscalls::channel_write(devmgr, &req[..16 + hlen], &[]).is_err() {
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    if syscalls::channel_read(devmgr, &mut resp, &mut handles).is_err() {
        return false;
    }
    i16::from_le_bytes([resp[0], resp[1]]) == 0
}

fn devmgr_read_reg(devmgr: usize, handle: u32, offset: u32) -> Option<u32> {
    let handle_str = {
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
    };
    let hlen = handle_str.iter().position(|&b| b == 0).unwrap_or(12);

    let off_str = {
        let mut nb = [0u8; 12];
        let mut i = 12;
        let mut v = offset as u64;
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
    };
    let olen = off_str.iter().position(|&b| b == 0).unwrap_or(12);

    let mut payload = [0u8; 32];
    let plen = hlen + 1 + olen;
    payload[..hlen].copy_from_slice(&handle_str[..hlen]);
    payload[hlen] = b' ';
    payload[hlen + 1..hlen + 1 + olen].copy_from_slice(&off_str[..olen]);

    let mut req = [0u8; 256];
    req[0] = DEV_MAGIC;
    req[1] = DEV_VER;
    req[2] = DEV_READ;
    req[4..8].copy_from_slice(&1u32.to_le_bytes());
    req[16..16 + plen].copy_from_slice(&payload[..plen]);

    if syscalls::channel_write(devmgr, &req[..16 + plen], &[]).is_err() {
        return None;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    let n = syscalls::channel_read(devmgr, &mut resp, &mut handles).ok()?;
    if n < 16 {
        return None;
    }
    let status = i16::from_le_bytes([resp[0], resp[1]]);
    if status != 0 {
        return None;
    }
    let plen = u32::from_le_bytes(resp[8..12].try_into().unwrap_or([0; 4])) as usize;
    if plen < 4 {
        return None;
    }
    Some(u32::from_le_bytes(resp[16..20].try_into().unwrap()))
}

fn devmgr_write_reg(devmgr: usize, handle: u32, offset: u32, value: u32) -> bool {
    let handle_str_bytes = {
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
    };
    let hlen = handle_str_bytes.iter().position(|&b| b == 0).unwrap_or(12);

    let off_str_bytes = {
        let mut nb = [0u8; 12];
        let mut i = 12;
        let mut v = offset as u64;
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
    };
    let olen = off_str_bytes.iter().position(|&b| b == 0).unwrap_or(12);

    let val_str_bytes = {
        let mut nb = [0u8; 12];
        let mut i = 12;
        let mut v = value as u64;
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
    };
    let vlen = val_str_bytes.iter().position(|&b| b == 0).unwrap_or(12);

    let mut payload = [0u8; 48];
    let mut pos = 0;
    payload[pos..pos + hlen].copy_from_slice(&handle_str_bytes[..hlen]);
    pos += hlen;
    payload[pos] = b' ';
    pos += 1;
    payload[pos..pos + olen].copy_from_slice(&off_str_bytes[..olen]);
    pos += olen;
    payload[pos] = b' ';
    pos += 1;
    payload[pos..pos + vlen].copy_from_slice(&val_str_bytes[..vlen]);
    pos += vlen;

    let mut req = [0u8; 256];
    req[0] = DEV_MAGIC;
    req[1] = DEV_VER;
    req[2] = DEV_WRITE;
    req[4..8].copy_from_slice(&1u32.to_le_bytes());
    req[16..16 + pos].copy_from_slice(&payload[..pos]);

    if syscalls::channel_write(devmgr, &req[..16 + pos], &[]).is_err() {
        return false;
    }
    let mut resp = [0u8; 256];
    let mut handles = [0u32; 2];
    if syscalls::channel_read(devmgr, &mut resp, &mut handles).is_err() {
        return false;
    }
    i16::from_le_bytes([resp[0], resp[1]]) == 0
}

fn u64_to_str(v: u64, buf: &mut [u8]) -> &str {
    if v == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap();
    }
    let mut i = buf.len();
    let mut n = v;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    core::str::from_utf8(&buf[i..]).unwrap()
}

pub fn do_open(session_idx: usize, path: &str, flags: u32) -> i32 {
    unsafe {
        let session_exists = SESSIONS[session_idx].is_some();
        if !session_exists {
            return -1;
        }

        if path == "dev/tty" || path == "/dev/tty" {
            return tty_fd() as i32;
        }

        if let Some(dev_name) = dev_name_from_path(path) {
            let sess = SESSIONS[session_idx].as_mut().unwrap();
            let devmgr = if sess.devmgr_chan != 0 {
                sess.devmgr_chan
            } else {
                match devmgr_connect() {
                    Some(ch) => {
                        sess.devmgr_chan = ch;
                        ch
                    }
                    None => return Status::NotFound.to_raw() as i32,
                }
            };

            let dev_handle = match devmgr_open(devmgr, dev_name) {
                Some(h) => h,
                None => return Status::NotFound.to_raw() as i32,
            };

            for fd in 0..MAX_FDS {
                if sess.fds[fd].is_none() {
                    sess.fds[fd] = Some(OpenFile {
                        node_idx: -1,
                        offset: 0,
                        dev_handle,
                        devmgr_chan: devmgr,
                    });
                    return fd as i32;
                }
            }
            return Status::NoMemory.to_raw() as i32;
        }

        let clean = if path.starts_with('/') { &path[1..] } else { path };
        let mut node = ramfs::resolve(clean);
        if node.is_none() && (flags & 0x40 != 0) {
            node = ramfs::create_file_path(clean);
        }
        match node {
            Some(idx) => {
                let sess = SESSIONS[session_idx].as_mut().unwrap();
                for fd in 0..MAX_FDS {
                    if sess.fds[fd].is_none() {
                        sess.fds[fd] = Some(OpenFile {
                            node_idx: idx,
                            offset: 0,
                            dev_handle: 0xFFFFFFFF,
                            devmgr_chan: 0,
                        });
                        return fd as i32;
                    }
                }
                Status::NoMemory.to_raw() as i32
            }
            None => Status::NotFound.to_raw() as i32,
        }
    }
}

pub fn do_close(session_idx: usize, fd: u32) -> i32 {
    unsafe {
        if fd == tty_fd() {
            return 0;
        }
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        if (fd as usize) < MAX_FDS {
            let of = &sess.fds[fd as usize];
            if let Some(of) = of {
                if of.devmgr_chan != 0 && of.dev_handle != 0xFFFFFFFF {
                    let _ = devmgr_close(of.devmgr_chan, of.dev_handle);
                }
            }
            sess.fds[fd as usize] = None;
            return 0;
        }
        -1
    }
}

pub fn do_read(session_idx: usize, fd: u32, buf: &mut [u8]) -> i32 {
    unsafe {
        if fd == tty_fd() {
            return 0;
        }
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let (is_dev, devmgr_chan, dev_handle, node_idx, offset) = {
            let of = match &sess.fds[fd as usize] {
                Some(f) => f,
                None => return -1,
            };
            (of.devmgr_chan != 0, of.devmgr_chan, of.dev_handle, of.node_idx, of.offset)
        };

        if is_dev {
            match devmgr_read_reg(devmgr_chan, dev_handle, offset) {
                Some(val) => {
                    let val_bytes = val.to_le_bytes();
                    let copy_len = val_bytes.len().min(buf.len());
                    buf[..copy_len].copy_from_slice(&val_bytes[..copy_len]);
                    if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                        of_mut.offset += 4;
                    }
                    copy_len as i32
                }
                None => Status::NotAllowed.to_raw() as i32,
            }
        } else {
            let result = ramfs::read(node_idx, buf, offset as usize);
            if result > 0 {
                if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                    of_mut.offset += result as u32;
                }
            }
            result
        }
    }
}

pub fn do_write(session_idx: usize, fd: u32, data: &[u8]) -> i32 {
    unsafe {
        if fd == tty_fd() {
            return data.len() as i32;
        }
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let (is_dev, devmgr_chan, dev_handle, node_idx, offset) = {
            let of = match &sess.fds[fd as usize] {
                Some(f) => f,
                None => return -1,
            };
            (of.devmgr_chan != 0, of.devmgr_chan, of.dev_handle, of.node_idx, of.offset)
        };

        if is_dev {
            if data.len() < 4 {
                return 0;
            }
            let val = u32::from_le_bytes(data[..4].try_into().unwrap());
            if devmgr_write_reg(devmgr_chan, dev_handle, offset, val) {
                if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                    of_mut.offset += 4;
                }
                data.len() as i32
            } else {
                Status::NotAllowed.to_raw() as i32
            }
        } else {
            let result = ramfs::write(node_idx, data, offset as usize);
            if result > 0 {
                if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                    of_mut.offset += result as u32;
                }
            }
            result
        }
    }
}

pub fn do_mkdir(path: &str) -> i32 {
    ramfs::mkdir_path(path)
}

pub fn do_rmdir(path: &str) -> i32 {
    let child = ramfs::resolve(path);
    match child {
        Some(idx) => {
            let parent = ramfs::parent_of(idx);
            if parent < 0 {
                Status::InvalidArgs.to_raw() as i32
            } else if ramfs::remove(parent, idx) {
                0
            } else {
                Status::NotEmpty.to_raw() as i32
            }
        }
        None => Status::NotFound.to_raw() as i32,
    }
}

pub fn do_unlink(path: &str) -> i32 {
    let child = ramfs::resolve(path);
    match child {
        Some(idx) => {
            let parent = ramfs::parent_of(idx);
            if parent < 0 {
                Status::InvalidArgs.to_raw() as i32
            } else if ramfs::remove(parent, idx) {
                0
            } else {
                -1
            }
        }
        None => Status::NotFound.to_raw() as i32,
    }
}

pub fn do_readdir(session_idx: usize, fd: u32, buf: &mut [u8]) -> i32 {
    unsafe {
        let sess = match SESSIONS[session_idx].as_mut() {
            Some(s) => s,
            None => return -1,
        };
        let of = match &sess.fds[fd as usize] {
            Some(f) => f,
            None => return -1,
        };
        let node_idx = of.node_idx;
        let result = ramfs::readdir_names(node_idx, buf);
        if result > 0 {
            if let Some(ref mut of_mut) = sess.fds[fd as usize] {
                of_mut.offset += result as u32;
            }
        }
        result
    }
}

pub fn do_stat(path: &str) -> (i32, u64) {
    let clean = if path.starts_with('/') { &path[1..] } else { path };
    let node = ramfs::resolve(clean);
    match node {
        Some(idx) => {
            let size = ramfs::stat_size(idx);
            let ntype = ramfs::stat_type(idx);
            let type_val = match ntype {
                Some(ramfs::NodeType::File) => 1u64,
                Some(ramfs::NodeType::Directory) => 2u64,
                None => 0u64,
            };
            (size, type_val)
        }
        None => (-1, 0),
    }
}
