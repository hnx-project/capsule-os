//! 2.0 Unified Memory Manager Object - Holds hardware page table translation and pure logic-level VMAR region tree.

use shared::status::{Result, Status};
use crate::arch::aarch64::page_table::PageTableTree;
use crate::memory::vmar::{Vmar, VmarFlags};
use crate::memory::vmo::Vmo;
use crate::memory::phys::PhysPage;
use crate::arch::aarch64::phys::PhysAddr;

#[derive(Debug)]
pub struct Memory {
    pub page_table: PageTableTree,
    pub root_vmar: Vmar,
    pt_pages: [Option<PhysPage>; 64],
    pt_pages_count: usize,
}

impl Memory {
    pub fn new_user_space(base: usize, size: usize) -> Result<Self> {
        let mut page_table = PageTableTree::new();
        page_table.allocate_root()?;

        let root_vmar = Vmar::create(base, size)?;
        let pt_pages = [const { None }; 64];

        Ok(Self {
            page_table,
            root_vmar,
            pt_pages,
            pt_pages_count: 0,
        })
    }

    pub fn new_dummy() -> Self {
        Self {
            page_table: PageTableTree::new(),
            root_vmar: Vmar::new_dummy(),
            pt_pages: [const { None }; 64],
            pt_pages_count: 0,
        }
    }

    pub fn track_pt_page(&mut self, pa: usize) {
        if self.pt_pages_count >= 64 { return; }
        for i in 0..self.pt_pages_count {
            if let Some(ref page) = self.pt_pages[i] {
                if page.as_usize() == pa { return; }
            }
        }
        let phys_page = PhysPage::new(PhysAddr::new(pa));
        self.pt_pages[self.pt_pages_count] = Some(phys_page);
        self.pt_pages_count += 1;
        self.page_table.track(pa);
    }

    pub fn map_vmo_segment(&mut self, vmo: &mut Vmo, vmo_offset: usize, virt_addr: usize, size: usize, flags: VmarFlags) -> Result<()> {
        self.root_vmar.reserve_mapping(vmo.id, vmo_offset, virt_addr, size, flags)?;

        let mut arch_flags = crate::arch::mmu::MapFlags::kernel_rw();
        arch_flags.readable = flags.readable() || flags.writable() || flags.executable();
        arch_flags.writable = flags.writable();
        arch_flags.executable = flags.executable();
        arch_flags.user = flags.user();

        let page_count = size / 4096;
        let mut mapped_pages = 0;

        if page_count > 100 {
            crate::log_info!("MMU", "map_vmo_segment: starting mapping of {} pages for VMO {}", page_count, vmo.id);
        }

        for i in 0..page_count {
            let vmo_off = vmo_offset + i * 4096;
            let va = virt_addr + i * 4096;

            if page_count > 100 && i % 200 == 0 {
                crate::log_info!("MMU", "map_vmo_segment: mapped {}/{} pages...", i, page_count);
            }

            match vmo.commit_page(vmo_off) {
                Ok(_) => {
                    let pa = vmo.get_page_phys(vmo_off).unwrap();
                    if let Err(e) = self.page_table.map_va(va, pa.as_usize(), &arch_flags) {
                        return Err(e);
                    }
                    mapped_pages += 1;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(())
    }
}
