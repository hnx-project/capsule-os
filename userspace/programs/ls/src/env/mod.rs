#[derive(Debug)]
pub enum FsError {
    DirectoryNotFound,
    PermissionDenied,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub enum FileType {
    Unknown = 0,
    RegularFile = 1,
    Directory = 2,
}

/// 定长且物理对齐的轻量级目录项设计，适合 no_std
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent {
    pub ino: u64,
    pub size: u64, // 新增大小字段，用于支持 -l 详细列表显示
    pub ftype: u8,
    pub name_len: u8,
    pub name: [u8; 110], // 定长 110 字节，使结构体大小正好为 128 字节 (8+8+1+1+110 = 128)
}

impl Dirent {
    pub fn new() -> Self {
        Self {
            ino: 0,
            size: 0,
            ftype: 0,
            name_len: 0,
            name: [0u8; 110],
        }
    }
}

pub trait FileSystem {
    fn open_dir(&self, path: &str) -> Result<i32, FsError>;
    /// 获取下一个目录项。如果读取完毕返回 Ok(false) 否则返回 Ok(true)
    fn readdir(&self, fd: i32, dirent: &mut Dirent) -> Result<bool, FsError>;
    fn close_dir(&self, fd: i32);
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
