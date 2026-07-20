use shared::status::Status;
use crate::arch::aarch64::phys::{PhysAddr, free_page};

/// A physical page owner that guarantees safe release and zero physical leaks.
/// Under 2.0 architecture, this acts as the sole representative of an allocated 4 KiB frame.
#[derive(Debug)]
pub struct PhysPage {
    pa: PhysAddr,
}

impl PhysPage {
    /// Wrapping a raw physical address into a safe RAII object.
    pub const fn new(pa: PhysAddr) -> Self {
        Self { pa }
    }

    /// Access the underlying raw physical address.
    pub fn addr(&self) -> PhysAddr {
        self.pa
    }

    /// Access the raw address as a usize.
    pub fn as_usize(&self) -> usize {
        self.pa.as_usize()
    }

    /// Release ownership of the physical page without freeing it (e.g. when moving into raw register space).
    pub fn leak(self) -> PhysAddr {
        let pa = self.pa;
        core::mem::forget(self);
        pa
    }
}

impl Drop for PhysPage {
    fn drop(&mut self) {
        if self.pa.as_usize() != 0 {
            free_page(self.pa);
        }
    }
}
