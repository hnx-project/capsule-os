pub const VFS_OPEN: u8 = 1;
pub const VFS_CLOSE: u8 = 2;
pub const VFS_READ: u8 = 3;
pub const VFS_READ_EXT: u8 = 4;
pub const VFS_WRITE: u8 = 5;
pub const VFS_MKDIR: u8 = 6;
pub const VFS_RMDIR: u8 = 7;
pub const VFS_UNLINK: u8 = 8;
pub const VFS_READDIR: u8 = 9;
pub const VFS_STAT: u8 = 10;

pub const INLINE_DATA_MAX: usize = 128;

#[repr(C)]
pub struct Request {
    pub command: u8,
    pub flags: u8,
    pub _reserved: [u8; 2],
    pub arg32_1: u32,
    pub arg32_2: u32,
    pub arg64: u64,
    pub data: [u8; INLINE_DATA_MAX],
}

impl Request {
    pub fn path(&self) -> &str {
        let len = self.data.iter().position(|&b| b == 0).unwrap_or(INLINE_DATA_MAX);
        core::str::from_utf8(&self.data[..len]).unwrap_or("")
    }
}

#[repr(C)]
pub struct Response {
    pub value: i64,
}

impl Response {
    pub const SIZE: usize = 8;

    pub fn ok(val: i64) -> [u8; Self::SIZE] {
        val.to_le_bytes()
    }

    pub fn err(status: shared::status::Status) -> [u8; Self::SIZE] {
        (status.to_raw() as i64).to_le_bytes()
    }
}
