//! Virtual Memory Object (VMO) - Redesigned with 2.0 RAII PhysPage principles.
//! Under the new architecture, VMO is fully platform-neutral and holds actual
//! logical ownership of physical pages.

use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use core::ops::Add;
use crate::arch::mmu_facade::pa_to_kernel_va;
use crate::arch::aarch64::phys::{self, PhysAddr};
use crate::memory::phys::PhysPage;

const VMO_MAGIC: u64 = 0x564D_4F4D_4147_4341;
const VMO_VERSION: u64 = 1;
const PAGE_SIZE: usize = 4096;
pub const VMO_MAX_PAGES: usize = 8192;

const PA_TABLE_SIZE: usize = 64;
const PA_TABLE_BASE: usize = PAGE_SIZE - PA_TABLE_SIZE; // 4032

static VMO_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[repr(C)]
struct VmoHeader {
    magic: u64,
    version: u64,
    capacity_pages: u64,
    committed: u64,
    size_bytes: u64,
    meta_page_count: u64,
}

#[derive(Debug)]
pub struct Vmo {
    pub id: u64,
    meta_page: PhysPage,
    size: usize,
    pub is_cow: bool,
    pub parent_id: Option<u64>,
    /// Array of allocated metadata pages for pages 1+ to keep their RAII live.
    extra_meta_pages: [Option<PhysPage>; 64],
    /// Track of committed RAII data pages.
    committed_pages: alloc::vec::Vec<Option<PhysPage>>,
}

fn header_size() -> usize { core::mem::size_of::<VmoHeader>() }
fn slot_size() -> usize { core::mem::size_of::<Option<PhysAddr>>() }
fn slots_per_page() -> usize { (PA_TABLE_BASE - header_size()) / slot_size() }
fn meta_pages_needed(page_count: usize) -> usize {
    let sp = slots_per_page();
    (page_count + sp - 1) / sp
}

impl Vmo {
    pub fn size(&self) -> usize { self.size }
    pub fn get_size(&self) -> usize { self.size }
    pub fn page_count(&self) -> usize { self.size / PAGE_SIZE }
    pub fn is_physical(&self) -> bool { self.committed_pages.is_empty() }

    pub fn create_physical(phys_addr: usize, size: usize) -> Result<Self> {
        if size == 0 { return Err(Status::InvalidArgs); }
        let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        if pages > VMO_MAX_PAGES { return Err(Status::InvalidArgs); }

        let mpn = meta_pages_needed(pages);
        crate::log_info!("VMO", "create_physical: addr={:#x}, size={}, pages={}, mpn={}", phys_addr, size, pages, mpn);
        let meta_pa = phys::alloc_vmo_meta()?;
        let meta_page = PhysPage::new(meta_pa);

        let mut extra_meta_pages = [const { None }; 64];
        let committed_pages = alloc::vec::Vec::new(); // 物理借用VMO，直接保持空Vec，零字节堆开销！

        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            let header = base as *mut VmoHeader;
            (*header).magic = VMO_MAGIC;
            (*header).version = VMO_VERSION;
            (*header).capacity_pages = pages as u64;
            (*header).committed = pages as u64;
            (*header).size_bytes = (pages * PAGE_SIZE) as u64;
            (*header).meta_page_count = mpn as u64;

            for i in 1..mpn {
                let extra_pa = phys::alloc_vmo_meta()?;
                let extra_page = PhysPage::new(extra_pa);
                let table_off = PA_TABLE_BASE + (i - 1) * 8;
                core::ptr::write_volatile(base.add(table_off) as *mut u64, extra_pa.as_usize() as u64);
                extra_meta_pages[i - 1] = Some(extra_page);
            }

            for i in 0..pages {
                let cur_pa = PhysAddr::new(phys_addr + i * PAGE_SIZE);
                // In physical/borrowed VMOs, pages are mapped directly but not owned by RAII to avoid double free.
                *slot_ptr(meta_pa.as_usize(), mpn, i) = Some(cur_pa);
            }
        }

        Ok(Vmo {
            id: VMO_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u64,
            meta_page,
            size: pages * PAGE_SIZE,
            is_cow: false,
            parent_id: None,
            extra_meta_pages,
            committed_pages,
        })
    }

    pub fn create_with_size(size: usize) -> Result<Self> {
        if size == 0 { return Err(Status::InvalidArgs); }
        let pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        if pages > VMO_MAX_PAGES { return Err(Status::InvalidArgs); }

        let mpn = meta_pages_needed(pages);
        let meta_pa = phys::alloc_vmo_meta()?;
        let meta_page = PhysPage::new(meta_pa);

        let mut extra_meta_pages = [const { None }; 64];
        let mut committed_pages = alloc::vec::Vec::with_capacity(pages);
        for _ in 0..pages {
            committed_pages.push(None);
        }

        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            let header = base as *mut VmoHeader;
            (*header).magic = VMO_MAGIC;
            (*header).version = VMO_VERSION;
            (*header).capacity_pages = pages as u64;
            (*header).committed = 0;
            (*header).size_bytes = (pages * PAGE_SIZE) as u64;
            (*header).meta_page_count = mpn as u64;

            for i in 1..mpn {
                let extra_pa = phys::alloc_vmo_meta()?;
                let extra_page = PhysPage::new(extra_pa);
                let table_off = PA_TABLE_BASE + (i - 1) * 8;
                core::ptr::write_volatile(base.add(table_off) as *mut u64, extra_pa.as_usize() as u64);
                extra_meta_pages[i - 1] = Some(extra_page);
            }
        }

        Ok(Vmo {
            id: VMO_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u64,
            meta_page,
            size: pages * PAGE_SIZE,
            is_cow: false,
            parent_id: None,
            extra_meta_pages,
            committed_pages,
        })
    }

    pub fn commit_page(&mut self, offset: usize) -> Result<Option<PhysAddr>> {
        if offset >= self.size { return Err(Status::InvalidArgs); }
        let page_idx = offset / PAGE_SIZE;
        
        if self.committed_pages.is_empty() {
            return Err(Status::NotAllowed);
        }
        
        if self.committed_pages[page_idx].is_some() {
            return Ok(None);
        }

        let pa = phys::alloc_vmo_data()?;
        let phys_page = PhysPage::new(pa);

        unsafe {
            let slot = self.page_slot(page_idx);
            *slot = Some(pa);
            (*self.header()).committed += 1;
        }

        self.committed_pages[page_idx] = Some(phys_page);
        Ok(Some(pa))
    }

    pub fn commit_all(&mut self) -> Result<()> {
        let n = self.size / PAGE_SIZE;
        for i in 0..n {
            self.commit_page(i * PAGE_SIZE)?;
        }
        Ok(())
    }

    pub fn fork(&mut self, new_id: u64) -> Result<Self> {
        let pages = self.size / PAGE_SIZE;
        let mpn = meta_pages_needed(pages);

        let mut fork_vmo = Self::create_with_size(self.size)?;
        fork_vmo.id = new_id;
        fork_vmo.is_cow = true;
        fork_vmo.parent_id = Some(self.id);

        if !self.committed_pages.is_empty() {
            for i in 0..pages {
                if let Some(old_page) = &self.committed_pages[i] {
                    // For CoW simple implementation: allocate new page and copy payload
                    fork_vmo.commit_page(i * PAGE_SIZE)?;
                    let mut buf = [0u8; 4096];
                    self.read(i * PAGE_SIZE, &mut buf)?;
                    fork_vmo.write(i * PAGE_SIZE, &buf)?;
                }
            }
        } else {
            // For physical parent VMO, we copy directly from its mapped physical pages!
            for i in 0..pages {
                if unsafe { *slot_ptr(self.meta_page.addr().as_usize(), mpn, i) }.is_some() {
                    fork_vmo.commit_page(i * PAGE_SIZE)?;
                    let mut buf = [0u8; 4096];
                    self.read(i * PAGE_SIZE, &mut buf)?;
                    fork_vmo.write(i * PAGE_SIZE, &buf)?;
                }
            }
        }

        Ok(fork_vmo)
    }

    pub fn create_child_slice(&self, new_id: u64, offset: usize, size: usize) -> Result<Self> {
        if offset & (PAGE_SIZE - 1) != 0 || size & (PAGE_SIZE - 1) != 0 {
            return Err(Status::InvalidArgs);
        }
        if offset + size > self.size { return Err(Status::InvalidArgs); }
        let pages = size / PAGE_SIZE;
        let start_page_idx = offset / PAGE_SIZE;

        let mut child_vmo = Self::create_with_size(size)?;
        child_vmo.id = new_id;
        child_vmo.parent_id = Some(self.id);

        unsafe {
            let header = child_vmo.header();
            (*header).committed = pages as u64;

            let mpn = meta_pages_needed(self.page_count());
            for i in 0..pages {
                if !self.committed_pages.is_empty() {
                    if let Some(ref parent_page) = self.committed_pages[start_page_idx + i] {
                        let pa = parent_page.addr();
                        // Slices do NOT own the pages through RAII to avoid double frees; they share parent's pages.
                        let slot = child_vmo.page_slot(i);
                        *slot = Some(pa);
                    }
                } else {
                    if let Some(pa) = *slot_ptr(self.meta_page.addr().as_usize(), mpn, start_page_idx + i) {
                        let slot = child_vmo.page_slot(i);
                        *slot = Some(pa);
                    }
                }
            }
        }

        Ok(child_vmo)
    }

    pub fn make_cow(&mut self) { self.is_cow = true; }

    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        if offset >= self.size { return Ok(0); }
        let mut written = 0;
        let mut cur = offset;
        while written < buf.len() && cur < self.size {
            let page_idx = cur / PAGE_SIZE;
            let in_page = cur % PAGE_SIZE;
            let pa = unsafe { *self.page_slot(page_idx) };
            let available = PAGE_SIZE - in_page;
            let want = core::cmp::min(available, buf.len() - written);
            let end = core::cmp::min(cur + want, self.size);
            let real = end - cur;
            match pa {
                Some(p) => unsafe {
                    let src = pa_to_kernel_va(p.as_usize()).add(in_page);
                    core::ptr::copy_nonoverlapping(src as *const u8, buf.as_mut_ptr().add(written), real);
                },
                None => {
                    for b in &mut buf[written..written + real] {
                        *b = 0;
                    }
                }
            }
            written += real;
            cur = end;
        }
        Ok(written)
    }

    pub fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize> {
        if offset >= self.size { return Ok(0); }
        let mut written = 0;
        let mut cur = offset;
        while written < buf.len() && cur < self.size {
            let page_idx = cur / PAGE_SIZE;
            let in_page = cur % PAGE_SIZE;
            if unsafe { (*self.page_slot(page_idx)).is_none() } {
                self.commit_page(cur & !(PAGE_SIZE - 1))?;
            }
            let pa = unsafe { (*self.page_slot(page_idx)).unwrap() };
            let available = PAGE_SIZE - in_page;
            let want = core::cmp::min(available, buf.len() - written);
            let end = core::cmp::min(cur + want, self.size);
            let real = end - cur;
            unsafe {
                let dst = pa_to_kernel_va(pa.as_usize()).add(in_page);
                core::ptr::copy_nonoverlapping(buf.as_ptr().add(written), dst as *mut u8, real);
                #[cfg(target_arch = "aarch64")]
                {
                    crate::arch::aarch64::mmu::sync_instruction_cache(dst as usize, real);
                }
            }
            written += real;
            cur = end;
        }
        Ok(written)
    }

    pub fn get_page_phys(&self, offset: usize) -> Option<PhysAddr> {
        if offset >= self.size { return None; }
        unsafe { *self.page_slot(offset / PAGE_SIZE) }
    }

    fn header(&self) -> *mut VmoHeader {
        pa_to_kernel_va(self.meta_page.as_usize()) as *mut VmoHeader
    }

    fn meta_page_count(&self) -> usize {
        unsafe { (*self.header()).meta_page_count as usize }
    }

    pub fn page_slot(&self, page_idx: usize) -> *mut Option<PhysAddr> {
        unsafe { slot_ptr(self.meta_page.as_usize(), self.meta_page_count(), page_idx) }
    }
}

unsafe fn slot_ptr(meta_pa: usize, mpn: usize, page_idx: usize) -> *mut Option<PhysAddr> {
    debug_assert!(page_idx < VMO_MAX_PAGES);
    let hs = header_size();
    let ss = slot_size();
    let spp = slots_per_page();

    let meta_idx = page_idx / spp;
    let slot_off = page_idx % spp;

    let pa = if meta_idx == 0 {
        meta_pa
    } else {
        let page0 = pa_to_kernel_va(meta_pa) as *const u8;
        let table_off = PA_TABLE_BASE + (meta_idx - 1) * 8;
        core::ptr::read_volatile(page0.add(table_off) as *const usize)
    };

    let offset = hs + slot_off * ss;
    let base = pa_to_kernel_va(pa) as *mut u8;
    base.add(offset) as *mut Option<PhysAddr>
}

impl Clone for Vmo {
    fn clone(&self) -> Self {
        // Safe shallow reference copying, RAII drops are avoided since the clone target copies metadata base PAs.
        // NOTE: Standard clone does shallow tracking; for deep copies, fork should be preferred.
        let mut extra_meta_pages = [const { None }; 64];
        
        for i in 0..64 {
            if let Some(ref page) = self.extra_meta_pages[i] {
                extra_meta_pages[i] = Some(PhysPage::new(page.addr()));
                core::mem::forget(page.addr()); // Avoid double release on the cloned meta pages
            }
        }

        let pages_len = self.size / PAGE_SIZE;
        let mut committed_pages = alloc::vec::Vec::new();
        if !self.committed_pages.is_empty() {
            committed_pages.reserve(pages_len);
            for i in 0..pages_len {
                if let Some(ref page) = self.committed_pages[i] {
                    committed_pages.push(Some(PhysPage::new(page.addr())));
                    core::mem::forget(page.addr()); // Avoid double release on data pages
                } else {
                    committed_pages.push(None);
                }
            }
        }

        let meta_page = PhysPage::new(self.meta_page.addr());
        core::mem::forget(self.meta_page.addr());

        Vmo {
            id: self.id,
            meta_page,
            size: self.size,
            is_cow: self.is_cow,
            parent_id: self.parent_id,
            extra_meta_pages,
            committed_pages,
        }
    }
}
