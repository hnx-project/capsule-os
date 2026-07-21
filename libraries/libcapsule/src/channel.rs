//! # 🔌 IPC Bidirectional Channel Wrapper Subsystem
//!
//! This module implements the high-level `Channel` abstraction.
//!
//! Channels are point-to-point, bidirectional IPC communication pipelines that are secure,
//! handle-transferred, and automatically cleaned up via RAII (`Drop`).

use shared::status::{Result, Status};

/// An RAII-style object capability for HNX double-ended communication Channels.
///
/// Automatically closes the underlying kernel handle when dropped to prevent
/// capability leaks.
///
/// # RAII Behavior
/// When a `Channel` goes out of scope, its `Drop` implementation automatically triggers
/// the `close` system call, alerting the peer end (`Status::PeerClosed`) and freeing kernel resources.
#[derive(Debug, PartialEq, Eq)]
pub struct Channel {
    handle: usize,
}

impl Channel {
    /// Wrap a raw channel handle value.
    ///
    /// # Parameters
    /// - `handle`: The raw `HandleValue` index assigned by the kernel.
    pub const fn from_raw(handle: usize) -> Self {
        Self { handle }
    }

    /// Expose the underlying raw kernel handle.
    pub const fn raw_handle(&self) -> usize {
        self.handle
    }

    /// Create a new connected pair of Channels. Returns `(client_channel, server_channel)` on success.
    ///
    /// # Errors
    /// - `Status::NoMemory`: If the kernel is unable to allocate descriptor slots or internal buffers.
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
    ///
    /// # Parameters
    /// - `name`: The registered namespace string to query (e.g. `"svc.vfs"`).
    ///
    /// # Errors
    /// - `Status::NotFound`: If no service has been registered under this name.
    pub fn lookup(name: &str) -> Result<Self> {
        match crate::syscalls::channel_lookup(name) {
            Ok(handle) => Ok(Self::from_raw(handle)),
            Err(e) => Err(e),
        }
    }

    /// Register this channel end in the global registry under a given name.
    ///
    /// Only privileged services/launchers are permitted to register services.
    ///
    /// # Parameters
    /// - `name`: The namespace name string.
    pub fn register(&self, name: &str) -> Result<()> {
        crate::syscalls::channel_register(name, self.handle)
    }

    /// Send a message through this channel. Can optionally attach handles to transfer.
    ///
    /// # Parameters
    /// - `data`: The byte buffer payload to transmit.
    /// - `attached_handles`: An array of raw capability handles to transfer atomically.
    ///
    /// # Errors
    /// - `Status::PeerClosed`: If the other end of the channel has been closed.
    /// - `Status::NotAllowed`: If writing is unauthorized on this handle.
    pub fn write(&self, data: &[u8], attached_handles: &[u32]) -> Result<usize> {
        crate::syscalls::channel_write(self.handle, data, attached_handles)
    }

    /// Receive a message from this channel. Populates data and returns actual size.
    ///
    /// # Parameters
    /// - `buf`: User-space destination buffer for the incoming byte payload.
    /// - `handles_out`: User-space destination buffer for any transferred capability handles.
    ///
    /// # Errors
    /// - `Status::PeerClosed`: If the peer has closed its end of the channel.
    /// - `Status::InvalidArgs`: If either buffer is invalid.
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
