use crate::os::capsule::Vmo;
use shared::status::Status;

/// Safer Object Wrapper for Process management
pub struct Process {
    pid: u64,
}

impl Process {
    /// Allocate a fresh sandboxed Process context inside the microkernel
    pub fn create(name: &str) -> Result<Self, Status> {
        let handle = libcapsule::syscalls::process_create(name)?;
        Ok(Self { pid: handle as u64 })
    }

    /// Retrieve the Process ID (PID)
    pub fn pid(&self) -> u64 {
        self.pid
    }

    /// Safely load an OHLINK executable binary from a slice VMO into the process
    pub fn load_binary(&self, binary_vmo: &Vmo, name: &str) -> Result<u64, Status> {
        let handle = binary_vmo.handle();
        libcapsule::syscalls::load_binary(handle, name, 0)
    }

    /// Atomically spawn and run a process using a binary VMO, abstracting away creation details.
    pub fn spawn(binary_vmo: &Vmo, name: &str) -> Result<u64, Status> {
        let handle = binary_vmo.handle();
        libcapsule::syscalls::load_binary(handle, name, 0)
    }
}
