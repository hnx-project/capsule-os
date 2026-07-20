pub const PROTOCOL_MAGIC: u8 = 0xD1;
pub const PROTOCOL_VERSION: u8 = 0x01;

pub const DEV_PROBE: u8 = 0x01;
pub const DEV_LIST: u8 = 0x02;
pub const DEV_INFO: u8 = 0x03;
pub const DEV_OPEN: u8 = 0x04;
pub const DEV_CLOSE: u8 = 0x05;
pub const DEV_READ: u8 = 0x06;
pub const DEV_WRITE: u8 = 0x07;

pub const REQUEST_HEADER_SIZE: usize = 16;
pub const RESPONSE_HEADER_SIZE: usize = 16;
pub const MAX_PAYLOAD_SIZE: usize = 240;
pub const MAX_MESSAGE_SIZE: usize = 256;

#[repr(C)]
pub struct RequestHeader {
    pub magic: u8,
    pub version: u8,
    pub command: u8,
    pub flags: u8,
    pub seq: u32,
    pub reserved: [u8; 8],
}

impl RequestHeader {
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < REQUEST_HEADER_SIZE {
            return None;
        }
        if buf[0] != PROTOCOL_MAGIC || buf[1] != PROTOCOL_VERSION {
            return None;
        }
        Some(RequestHeader {
            magic: buf[0],
            version: buf[1],
            command: buf[2],
            flags: buf[3],
            seq: u32::from_le_bytes(buf[4..8].try_into().ok()?),
            reserved: {
                let mut r = [0u8; 8];
                r.copy_from_slice(&buf[8..16]);
                r
            },
        })
    }
}

pub fn build_response(seq: u32, status: i16, payload: &[u8]) -> [u8; MAX_MESSAGE_SIZE] {
    let mut buf = [0u8; MAX_MESSAGE_SIZE];
    let pl_len = payload.len().min(MAX_PAYLOAD_SIZE) as u32;
    buf[..2].copy_from_slice(&status.to_le_bytes());
    buf[2..4].copy_from_slice(&[0u8; 2]);
    buf[4..8].copy_from_slice(&seq.to_le_bytes());
    buf[8..12].copy_from_slice(&pl_len.to_le_bytes());
    buf[12..16].copy_from_slice(&[0u8; 4]);
    if pl_len > 0 {
        buf[16..16 + pl_len as usize].copy_from_slice(&payload[..pl_len as usize]);
    }
    buf
}

pub fn response_status(resp: &[u8]) -> i16 {
    if resp.len() < 2 {
        return -1;
    }
    i16::from_le_bytes([resp[0], resp[1]])
}

pub fn response_payload<'a>(resp: &'a [u8]) -> &'a [u8] {
    if resp.len() < 16 {
        return &[];
    }
    let pl_len = u32::from_le_bytes(resp[8..12].try_into().unwrap_or([0; 4])) as usize;
    let end = 16 + pl_len;
    if end > resp.len() {
        return &resp[16..];
    }
    &resp[16..end]
}
