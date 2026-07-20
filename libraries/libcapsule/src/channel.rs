use shared::status::{Result, Status};

/// An RAII-style object capability for HNX double-ended communication Channels.
/// Automatically closes the underlying kernel handle when dropped to prevent
/// capability leaks.
#[derive(Debug, PartialEq, Eq)]
pub struct Channel {
    handle: usize,
}

impl Channel {
    /// Wrap a raw channel handle value.
    pub const fn from_raw(handle: usize) -> Self {
        Self { handle }
    }

    /// Expose the underlying raw kernel handle.
    pub const fn raw_handle(&self) -> usize {
        self.handle
    }

    /// Create a new connected pair of Channels. Returns (h0, h1) on success.
    pub fn create() -> Result<(Self, Self)> {
        match crate::syscalls::channel_create() {
            Ok(packed) => {
                let h0 = packed & 0xFFFF_FFFF;
                let h1 = packed >> 32;
                Ok((Self::from_raw(h0), Self::from_raw(h1)))
            }
            Err(e) => Err(e),
        }
    }

    /// Look up a service channel by name in the global registry.
    pub fn lookup(name: &str) -> Result<Self> {
        match crate::syscalls::channel_lookup(name) {
            Ok(handle) => Ok(Self::from_raw(handle)),
            Err(e) => Err(e),
        }
    }

    /// Register this channel end in the global registry under a given name.
    pub fn register(&self, name: &str) -> Result<()> {
        crate::syscalls::channel_register(name, self.handle)
    }

    /// Send a message through this channel. Can optionally attach handles to transfer.
    pub fn write(&self, data: &[u8], attached_handles: &[u32]) -> Result<usize> {
        crate::syscalls::channel_write(self.handle, data, attached_handles)
    }

    /// Receive a message from this channel. Populates data and returns actual size.
    pub fn read(&self, buf: &mut [u8], handles_out: &mut [u32]) -> Result<usize> {
        crate::syscalls::channel_read(self.handle, buf, handles_out)
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // Automatically close the capability to avoid resource leaks in the kernel.
        if self.handle != 0 {
            let _ = crate::syscalls::close(self.handle);
        }
    }
}
