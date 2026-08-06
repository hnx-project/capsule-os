pub mod phys;
pub mod vmo;
pub mod vmar;
pub mod smoke;

pub use crate::memory::vmar::VmarFlags;
pub use crate::memory::vmo::Vmo;
pub use crate::memory::phys::PhysPage;
pub use crate::memory::vmar::Vmar;

// Re-export Memory manager
mod memory_manager;
pub use self::memory_manager::Memory;
