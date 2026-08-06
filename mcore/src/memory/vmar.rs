//! Virtual Memory Address Region (VMAR) - 2.0 Pure logical region allocator without direct hardware references.

use shared::status::{Result, Status};
use crate::arch::mmu::PAGE_SIZE;
use crate::arch::aarch64::phys::PhysAddr;
use crate::memory::phys::PhysPage;
use crate::memory::vmo::Vmo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmarFlags(u32);

impl VmarFlags {
    pub const NONE: VmarFlags = VmarFlags(0);
    pub const READ: VmarFlags = VmarFlags(1 << 0);
    pub const WRITE: VmarFlags = VmarFlags(1 << 1);
    pub const EXECUTE: VmarFlags = VmarFlags(1 << 2);
    pub const USER: VmarFlags = VmarFlags(1 << 3);

    pub const fn from_bits(b: u32) -> Self { VmarFlags(b) }
    pub const fn bits(&self) -> u32 { self.0 }

    pub fn readable(&self)   -> bool { self.0 & Self::READ.0    != 0 }
    pub fn writable(&self)   -> bool { self.0 & Self::WRITE.0   != 0 }
    pub fn executable(&self) -> bool { self.0 & Self::EXECUTE.0 != 0 }
    pub fn user(&self)       -> bool { self.0 & Self::USER.0    != 0 }
}

const MAX_CHILDREN: usize = 32;
const MAX_MAPPINGS: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    pub vmo_id: u64,
    pub vmo_offset: usize,
    pub virt_addr: usize,
    pub size: usize,
    pub flags: VmarFlags,
    pub installed: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SubRegion {
    pub base: usize,
    pub size: usize,
    pub meta_pa: Option<PhysAddr>,
}

#[derive(Debug)]
pub struct Vmar {
    pub base: usize,
    pub size: usize,
    meta_page: PhysPage,
    children: alloc::vec::Vec<SubRegion>,
    mappings: alloc::vec::Vec<Mapping>,
}

impl Vmar {
    pub const fn new_dummy() -> Self {
        Vmar {
            base: 0,
            size: 0,
            meta_page: PhysPage::new(PhysAddr::new(0)),
            children: alloc::vec::Vec::new(),
            mappings: alloc::vec::Vec::new(),
        }
    }

    pub fn create(base: usize, size: usize) -> Result<Self> {
        if base & (PAGE_SIZE - 1) != 0 || size == 0 {
            return Err(Status::InvalidArgs);
        }
        let size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let meta_pa = crate::arch::aarch64::phys::alloc_vmar_meta()?;
        let meta_page = PhysPage::new(meta_pa);

        Ok(Vmar {
            base,
            size,
            meta_page,
            children: alloc::vec::Vec::new(),
            mappings: alloc::vec::Vec::new(),
        })
    }

    pub fn allocate_subregion(&mut self, size: usize) -> Result<Vmar> {
        if size == 0 { return Err(Status::InvalidArgs); }
        let size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        if self.children.len() >= MAX_CHILDREN {
            return Err(Status::NoMemory);
        }

        let mut candidate = self.base;
        'outer: loop {
            if candidate + size > self.base + self.size {
                return Err(Status::NoMemory);
            }
            for c in self.children.iter() {
                if c.meta_pa.is_some() {
                    if candidate < c.base + c.size && candidate + size > c.base {
                        candidate = (c.base + c.size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                        continue 'outer;
                    }
                }
            }
            for m in self.mappings.iter() {
                if candidate < m.virt_addr + m.size && candidate + size > m.virt_addr {
                    candidate = (m.virt_addr + m.size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
                    continue 'outer;
                }
            }
            break;
        }

        let child = Vmar::create(candidate, size)?;
        self.children.push(SubRegion {
            base: candidate,
            size,
            meta_pa: Some(child.meta_page.addr()),
        });
        Ok(child)
    }

    /// Perform a logical mapping reservation.
    /// Actual hardware page table updates are decoupled and delegated to `Memory::map_vmo_segment()`.
    pub fn reserve_mapping(&mut self, vmo_id: u64, vmo_offset: usize, virt_addr: usize, size: usize, flags: VmarFlags) -> Result<()> {
        if size == 0 || virt_addr & (PAGE_SIZE - 1) != 0 || vmo_offset & (PAGE_SIZE - 1) != 0 {
            crate::log_error!(
                "VMAR",
                "reserve_mapping check 1 failed: size={}, virt_addr={:#x}, vmo_offset={:#x}",
                size, virt_addr, vmo_offset
            );
            return Err(Status::InvalidArgs);
        }
        if virt_addr < self.base || virt_addr + size > self.base + self.size {
            crate::log_error!(
                "VMAR",
                "reserve_mapping check 2 failed: virt_addr={:#x}, size={}, self.base={:#x}, self.size={:#x}",
                virt_addr, size, self.base, self.size
            );
            return Err(Status::InvalidArgs);
        }
        if self.mappings.len() >= MAX_MAPPINGS {
            return Err(Status::NoMemory);
        }

        self.mappings.push(Mapping {
            vmo_id,
            vmo_offset,
            virt_addr,
            size,
            flags,
            installed: true,
        });
        Ok(())
    }

    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }
}
