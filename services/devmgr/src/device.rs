use shared::device_info::DEVICE_RECORD_SIZE;
use shared::status::Status;

pub const MAX_DEVICES: usize = 8;
pub const MAX_SESSIONS: usize = 8;
pub const MAX_OPEN_PER_SESSION: usize = 8;

pub struct DeviceEntry {
    pub dev_type: u8,
    pub name: [u8; 16],
    pub base: u64,
    pub size: u64,
    pub irq: u32,
}

pub struct OpenHandle {
    pub device_idx: usize,
}

pub struct Session {
    pub handles: [Option<OpenHandle>; MAX_OPEN_PER_SESSION],
}

pub struct DeviceTable {
    pub devices: [Option<DeviceEntry>; MAX_DEVICES],
    pub count: usize,
}

pub static mut DEVICE_TABLE: DeviceTable = DeviceTable {
    devices: [const { None }; MAX_DEVICES],
    count: 0,
};

pub static mut SESSIONS: [Option<Session>; MAX_SESSIONS] = [const { None }; MAX_SESSIONS];

pub fn parse_kernel_buffer(buf: &[u8]) -> i32 {
    if buf.len() < 4 {
        return Status::InvalidArgs.to_raw() as i32;
    }
    let n = buf.len();
    let count = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
    let actual = count.min(MAX_DEVICES);
    unsafe {
        let table = &mut DEVICE_TABLE;
        table.count = 0;
        for i in 0..actual {
            let offset = 4 + i * DEVICE_RECORD_SIZE;
            if offset + DEVICE_RECORD_SIZE > n {
                break;
            }
            let rec = &buf[offset..offset + DEVICE_RECORD_SIZE];
            let dev_type = rec[0];
            let mut name = [0u8; 16];
            name.copy_from_slice(&rec[1..17]);
            let base = u64::from_le_bytes(rec[17..25].try_into().unwrap());
            let size = u64::from_le_bytes(rec[25..33].try_into().unwrap());
            let irq = u32::from_le_bytes(rec[33..37].try_into().unwrap());
            table.devices[table.count] = Some(DeviceEntry {
                dev_type,
                name,
                base,
                size,
                irq,
            });
            table.count += 1;
        }
    }
    0
}

pub fn device_name(entry: &DeviceEntry) -> &str {
    let end = entry.name.iter().position(|&b| b == 0).unwrap_or(16);
    core::str::from_utf8(&entry.name[..end]).unwrap_or("?")
}

pub fn list_device_names(buf: &mut [u8]) -> usize {
    unsafe {
        let table = &DEVICE_TABLE;
        let mut written = 0usize;
        for i in 0..table.count {
            if let Some(ref dev) = table.devices[i] {
                if written > 0 && written < buf.len() {
                    buf[written] = b'\n';
                    written += 1;
                }
                let name = device_name(dev);
                let name_bytes = name.as_bytes();
                let remain = buf.len().saturating_sub(written);
                let copy_len = name_bytes.len().min(remain);
                buf[written..written + copy_len].copy_from_slice(&name_bytes[..copy_len]);
                written += copy_len;
            }
        }
        written
    }
}

pub fn find_device(name: &str) -> Option<usize> {
    unsafe {
        let table = &DEVICE_TABLE;
        for i in 0..table.count {
            if let Some(ref dev) = table.devices[i] {
                if device_name(dev) == name {
                    return Some(i);
                }
            }
        }
        None
    }
}

fn u64_to_str(n: u64, buf: &mut [u8]) -> &str {
    if n == 0 {
        buf[0] = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap();
    }
    let mut i = buf.len();
    let mut v = n;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    core::str::from_utf8(&buf[i..buf.len()]).unwrap()
}

fn write_line(buf: &mut [u8], pos: &mut usize, label: &str, val: &str) -> Option<()> {
    let label_bytes = label.as_bytes();
    let val_bytes = val.as_bytes();
    let total = label_bytes.len() + 1 + val_bytes.len() + 1;
    if *pos + total > buf.len() {
        return None;
    }
    buf[*pos..*pos + label_bytes.len()].copy_from_slice(label_bytes);
    *pos += label_bytes.len();
    buf[*pos] = b' ';
    *pos += 1;
    buf[*pos..*pos + val_bytes.len()].copy_from_slice(val_bytes);
    *pos += val_bytes.len();
    buf[*pos] = b'\n';
    *pos += 1;
    Some(())
}

pub fn encode_device_info(idx: usize, buf: &mut [u8]) -> Option<usize> {
    unsafe {
        let table = &DEVICE_TABLE;
        let dev = table.devices.get(idx)?;
        let dev = dev.as_ref()?;

        let mut pos = 0usize;
        let mut num_buf = [0u8; 24];

        write_line(buf, &mut pos, "type", u64_to_str(dev.dev_type as u64, &mut num_buf))?;
        write_line(buf, &mut pos, "base", u64_to_str(dev.base, &mut num_buf))?;
        write_line(buf, &mut pos, "size", u64_to_str(dev.size, &mut num_buf))?;
        let irq_str = if dev.irq == 0xFFFFFFFF {
            "none"
        } else {
            u64_to_str(dev.irq as u64, &mut num_buf)
        };
        write_line(buf, &mut pos, "irq", irq_str)?;
        write_line(buf, &mut pos, "name", device_name(dev))?;

        Some(pos)
    }
}

pub fn find_free_session() -> Option<usize> {
    unsafe {
        for i in 0..MAX_SESSIONS {
            if SESSIONS[i].is_none() {
                return Some(i);
            }
        }
        None
    }
}

pub fn session_open(session_idx: usize, device_idx: usize) -> Option<u32> {
    unsafe {
        let sess = SESSIONS[session_idx].as_mut()?;
        for fd in 0..MAX_OPEN_PER_SESSION {
            if sess.handles[fd].is_none() {
                sess.handles[fd] = Some(OpenHandle { device_idx });
                return Some(fd as u32);
            }
        }
        None
    }
}

pub fn session_close(session_idx: usize, handle: u32) -> bool {
    unsafe {
        match SESSIONS[session_idx].as_mut() {
            Some(sess) => {
                let h = handle as usize;
                if h >= MAX_OPEN_PER_SESSION {
                    return false;
                }
                sess.handles[h] = None;
                true
            }
            None => false,
        }
    }
}

pub fn session_cleanup(session_idx: usize) {
    unsafe {
        SESSIONS[session_idx] = None;
    }
}

pub fn get_device_for_handle(session_idx: usize, handle: u32) -> Option<&'static DeviceEntry> {
    unsafe {
        let sess = SESSIONS[session_idx].as_ref()?;
        let h = handle as usize;
        if h >= MAX_OPEN_PER_SESSION {
            return None;
        }
        let oh = sess.handles[h].as_ref()?;
        get_dev_entry(oh.device_idx)
    }
}

pub fn get_dev_entry(idx: usize) -> Option<&'static DeviceEntry> {
    unsafe {
        let table = &DEVICE_TABLE;
        let dev = table.devices.get(idx)?;
        dev.as_ref()
    }
}
