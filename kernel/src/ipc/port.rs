use shared::status::{Result, Status};
use core::sync::atomic::AtomicU32;

static PORT_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
pub struct Port { pub id: u32, pub queue_len: u32 }

impl Port {
    pub fn new(queue_len: u32) -> Result<Self> {
        if queue_len == 0 || queue_len > 64 { return Err(Status::InvalidArgs); }
        Ok(Port { id: PORT_ID_COUNTER.fetch_add(1, Ordering::Relaxed), queue_len })
    }
    pub fn wait(&self) -> Result<PortPacket> { Err(Status::TimedOut) }
    pub fn queue(&self, _packet: &PortPacket) -> Result<()> { Ok(()) }
}

use core::sync::atomic::Ordering;
#[derive(Debug)]
pub struct PortPacket { pub key: u64, pub trigger: u32, pub status: i32, pub bytes: [u8; 64] }
