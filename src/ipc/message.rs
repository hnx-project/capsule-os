use shared::types::HandleValue;

#[derive(Debug)]
pub struct Message {
    pub data: &'static [u8],
    pub handles: &'static [HandleValue],
}

impl Message {
    pub fn new() -> Self { Message { data: &[], handles: &[] } }
}

impl Default for Message { fn default() -> Self { Self::new() } }

#[derive(Debug, Clone, Copy, Default)]
pub struct MessageMetadata { pub flags: u32, pub num_handles: u32 }
