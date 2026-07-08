pub mod vmo;
pub mod vmar;
pub mod phys;
pub mod elf;
pub mod mmu;
pub mod slab;
pub mod smoke;

pub use vmo::Vmo;
pub use vmar::Vmar;
pub use elf::ElfLoader;

pub fn init(ram_base: usize, ram_size: usize) { phys::init(ram_base, ram_size); }
