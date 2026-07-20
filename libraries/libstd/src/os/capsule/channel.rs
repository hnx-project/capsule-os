use shared::status::Status;

/// Safer Object Wrapper for IPC Channels
pub struct Channel {
    handle: usize,
}

impl Channel {
    /// Wrapping an existing raw Channel handle
    pub const unsafe fn from_raw_handle(handle: usize) -> Self {
        Self { handle }
    }

    /// Retrieve the underlying raw handle value
    pub fn handle(&self) -> usize {
        self.handle
    }

    /// Query the global registry for a named port/channel
    pub fn lookup(name: &str) -> Result<Self, Status> {
        let handle = libcapsule::syscalls::channel_lookup(name)?;
        unsafe { Ok(Self::from_raw_handle(handle)) }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        let _ = libcapsule::syscalls::close(self.handle);
    }
}
