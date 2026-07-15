use shared::launcher::ServiceDescriptor;
use shared::status::Status;

/// POSIX-like command structure to describe a service to be launched via the kernel.
pub struct Command<'a> {
    desc: ServiceDescriptor<'a>,
}

impl<'a> Command<'a> {
    /// Create a new service Command describing name and its exact BootFS path.
    pub fn new(name: &'a str, path: &'a str) -> Self {
        Self {
            desc: ServiceDescriptor {
                name,
                path,
                bootstrap_vmo_handle_index: None,
            },
        }
    }

    /// Set an optional bootstrap raw handle index (like index 100 for RootFS hand-off)
    pub fn bootstrap_vmo_index(mut self, index: u32) -> Self {
        self.desc.bootstrap_vmo_handle_index = Some(index);
        self
    }

    /// Spawns the described service into execution by calling the kernel service_spawn syscall.
    pub fn spawn(&self) -> Result<u64, Status> {
        libcapsule::syscalls::service_spawn(&self.desc)
    }
}
