use shared::status::{Result, Status};
use core::sync::atomic::{AtomicU32, Ordering};

static CHANNEL_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
pub struct Channel {
    pub id: u32,
    pub state: ChannelState,
    pub read_refs: AtomicU32,
    pub write_refs: AtomicU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState { Open, HalfClosed, Closed }

impl Channel {
    pub fn new() -> Result<Self> {
        Ok(Channel { id: CHANNEL_ID_COUNTER.fetch_add(1, Ordering::Relaxed), state: ChannelState::Open, read_refs: AtomicU32::new(1), write_refs: AtomicU32::new(1) })
    }
    pub fn read(&self, _data: &mut [u8], _handles: &mut [HandleValue]) -> Result<usize> { Ok(0) }
    pub fn write(&self, _data: &[u8], _handles: &[HandleValue]) -> Result<usize> { Ok(0) }
    pub fn close(&mut self) { self.state = ChannelState::Closed; }
}

use shared::types::HandleValue;
