pub const FATFS_MAX_VOLUMES: usize = 4;

#[derive(Debug, Clone, Copy)]
pub enum FatfsError {
    NotFound,
    AlreadyExists,
    NoSpace,
    InvalidArgs,
    IoError,
}

pub type FatfsResult<T> = core::result::Result<T, FatfsError>;

#[derive(Debug, Clone)]
pub struct FatfsVolume {
    pub drive_letter: u8,
    pub sector_size: u32,
    pub total_sectors: u64,
}

impl FatfsVolume {
    pub fn new(drive_letter: u8) -> Self {
        FatfsVolume {
            drive_letter,
            sector_size: 512,
            total_sectors: 0,
        }
    }

    pub fn mount(&mut self, _device_id: u32) -> FatfsResult<()> {
        Ok(())
    }

    pub fn unmount(&mut self) {}
}

pub struct Fatfs {
    volumes: [Option<FatfsVolume>; FATFS_MAX_VOLUMES],
}

impl Fatfs {
    pub fn new() -> Self {
        Fatfs {
            volumes: [const { None }; FATFS_MAX_VOLUMES],
        }
    }

    pub fn mount_volume(&mut self, drive_letter: u8, device_id: u32) -> FatfsResult<()> {
        for vol in &mut self.volumes {
            if vol.is_none() {
                let mut v = FatfsVolume::new(drive_letter);
                v.mount(device_id)?;
                *vol = Some(v);
                return Ok(());
            }
        }
        Err(FatfsError::NoSpace)
    }

    pub fn unmount_volume(&mut self, drive_letter: u8) -> FatfsResult<()> {
        for vol in &mut self.volumes {
            if let Some(ref mut v) = vol {
                if v.drive_letter == drive_letter {
                    v.unmount();
                    *vol = None;
                    return Ok(());
                }
            }
        }
        Err(FatfsError::NotFound)
    }

    pub fn open(&self, _path: &str, _mode: u32) -> FatfsResult<u32> {
        Err(FatfsError::NotFound)
    }

    pub fn close(&self, _fd: u32) -> FatfsResult<()> {
        Ok(())
    }

    pub fn read(&self, _fd: u32, _buf: &mut [u8]) -> FatfsResult<usize> {
        Err(FatfsError::IoError)
    }

    pub fn write(&self, _fd: u32, _buf: &[u8]) -> FatfsResult<usize> {
        Err(FatfsError::IoError)
    }

    pub fn seek(&self, _fd: u32, _offset: i64, _whence: i32) -> FatfsResult<u64> {
        Err(FatfsError::IoError)
    }

    pub fn mkdir(&self, _path: &str) -> FatfsResult<()> {
        Err(FatfsError::NotFound)
    }

    pub fn rmdir(&self, _path: &str) -> FatfsResult<()> {
        Err(FatfsError::NotFound)
    }

    pub fn unlink(&self, _path: &str) -> FatfsResult<()> {
        Err(FatfsError::NotFound)
    }

    pub fn readdir(&self, _dir_fd: u32, _buf: &mut [u8]) -> FatfsResult<usize> {
        Err(FatfsError::IoError)
    }
}
