pub mod virtio_blk;

use shared::status::Result;

pub trait BlockDriver: Send + Sync {
    fn read_sectors(&self, sector: u64, dst_pa: usize) -> Result<()>;
    fn write_sectors(&self, sector: u64, src_pa: usize) -> Result<()>;
    fn get_capacity(&self) -> u64;
}

pub static ACTIVE_BLOCK_DEVICE: spin::Mutex<Option<&'static dyn BlockDriver>> = spin::Mutex::new(None);
