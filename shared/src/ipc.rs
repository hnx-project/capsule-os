#![no_std]

#[derive(Debug, Clone, Copy)]
pub struct Message;

impl Message {
    pub fn new() -> Self { Message }
}

impl Default for Message { fn default() -> Self { Self::new() } }

#[derive(Debug, Clone, Copy, Default)]
pub struct MessageMetadata { pub flags: u32, pub num_handles: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelEndpoint { Left, Right }
