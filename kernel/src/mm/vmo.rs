//! Virtual Memory Object (VMO).
//!
//! A VMO is a kernel-managed collection of physical pages that can be mapped
//! into any VMAR.  This is a stripped-down Zircon-style VMO; the only kind
//! of backing store we support is anonymous (contiguous-in-`PhysAddr`-index
//! space, not necessarily contiguous in physical address space).
//!
//! ## On-disk layout of metadata
//!
//! Each VMO owns one or more metadata pages allocated from the physical
//! page allocator.  The first metadata page stores a `VmoHeader` followed
//! by a PA table listing the physical addresses of any additional metadata
//! pages, followed by the page-slot array for the first batch of data-page
//! pointers.  Additional metadata pages carry only page-slot entries.
//!
//! The header stores `meta_page_count` — the total number of metadata
//! pages allocated for this VMO.  `page_slot()` walks the PA table to
//! find the correct physical page for any slot index, so metadata pages
//! need NOT be physically contiguous.
//!
//! ## Page layout (all metadata pages)
//!
//! ```text
//! page 0:
//!   [0..47]    VmoHeader
//!   [48..4031] Page slots (249 entries)
//!   [4032..4095] PA table (8 entries × 8 bytes)
//!
//! pages 1+:
//!   [0..47]    Reserved (zeroed, unused)
//!   [48..4031] Page slots (249 entries)
//!   [4032..4095] Reserved (zeroed, unused)
//! ```

use core::sync::atomic::{AtomicUsize, Ordering};

use shared::status::{Result, Status};

use core::ops::Add;

use crate::mm::mmu::pa_to_kernel_va;
use crate::mm::phys::{self, PhysAddr};

const VMO_MAGIC: u64 = 0x564D_4F4D_4147_4341; // "VMOMAGCA" (visible in hex dumps)
const VMO_VERSION: u64 = 1;
const PAGE_SIZE: usize = 4096;
/// Maximum number of pages a single VMO can address.
pub const VMO_MAX_PAGES: usize = 2048;

/// Size of the PA table at the end of each metadata page.
/// 8 entries × 8 bytes = 64 bytes, enough for 8 extra metadata pages
/// (covering ceil(2048/249) + 1 = 9 total metadata pages).
const PA_TABLE_SIZE: usize = 64;
/// Byte offset of the PA table within a metadata page.
const PA_TABLE_BASE: usize = PAGE_SIZE - PA_TABLE_SIZE; // 4032

static VMO_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Header stored at the start of every metadata page.
///
/// Layout (48 bytes):
///   [0..7]   magic
///   [8..15]  version
///   [16..23] capacity_pages
///   [24..31] committed
///   [32..39] size_bytes
///   [40..47] meta_page_count
///
/// Only the first metadata page's header is meaningful; on extra pages
/// the header area is zeroed and unused.  The PA table for additional
/// metadata pages lives at the end of page 0 (see module docs).
#[repr(C)]
struct VmoHeader {
    magic: u64,
    version: u64,
    capacity_pages: u64,
    /// Number of pages currently committed (allocated physical pages).
    committed: u64,
    /// Size in bytes (capacity * 4096).
    size_bytes: u64,
    /// Total number of metadata pages allocated for this VMO.
    meta_page_count: u64,
}

/// The user-facing VMO handle.
#[derive(Debug)]
pub struct Vmo {
    pub id: u64,
    /// Physical address of the first metadata page.
    meta_pa: PhysAddr,
    /// Total size in bytes (always a multiple of PAGE_SIZE).
    size: usize,
    pub is_cow: bool,
    pub parent_id: Option<u64>,
}

// --- helper constants / functions ---

fn header_size() -> usize {
    core::mem::size_of::<VmoHeader>()
}

fn slot_size() -> usize {
    core::mem::size_of::<Option<PhysAddr>>()
}

fn slots_per_page() -> usize {
    // Page 0 has the PA table at the end, so slots occupy
    // header_size() .. PA_TABLE_BASE.  Extra pages are laid
    // out identically (the PA table area is unused there).
    (PA_TABLE_BASE - header_size()) / slot_size()
}

fn meta_pages_needed(page_count: usize) -> usize {
    let sp = slots_per_page();
    (page_count + sp - 1) / sp
}

impl Vmo {
    /// Total size in bytes (always a multiple of PAGE_SIZE).
    pub fn size(&self) -> usize {
        self.size
    }

    /// Create a VMO with `size` bytes capacity.  No physical pages are
    /// committed yet; callers must call `commit_page(offset)` (or
    /// `commit_all()`) before reading/writing.
    pub fn create_with_size(size: usize) -> Result<Self> {
        if size == 0 {
            return Err(Status::InvalidArgs);
        }
        let pages = round_up_to_pages(size);
        if pages > VMO_MAX_PAGES {
            return Err(Status::InvalidArgs);
        }

        let hs = header_size();
        let mpn = meta_pages_needed(pages);

        // Allocate the first metadata page and write the header.
        let meta_pa = phys::alloc_page()?;
        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            for i in 0..PAGE_SIZE {
                core::ptr::write_volatile(base.add(i), 0);
            }
            let header = base as *mut VmoHeader;
            (*header).magic = VMO_MAGIC;
            (*header).version = VMO_VERSION;
            (*header).capacity_pages = pages as u64;
            (*header).committed = 0;
            (*header).size_bytes = (pages * PAGE_SIZE) as u64;
            (*header).meta_page_count = mpn as u64;

            // Allocate remaining metadata pages and record their PAs
            // in the PA table at the end of page 0.
            for i in 1..mpn {
                let extra_pa = phys::alloc_page()?;
                let table_off = PA_TABLE_BASE + (i - 1) * 8;
                core::ptr::write_volatile(
                    base.add(table_off) as *mut u64,
                    extra_pa.as_usize() as u64,
                );
                // Zero the extra metadata page.
                let eb = pa_to_kernel_va(extra_pa.as_usize()) as *mut u8;
                for j in 0..PAGE_SIZE {
                    core::ptr::write_volatile(eb.add(j), 0);
                }
            }
        }

        Ok(Vmo {
            id: VMO_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u64,
            meta_pa,
            size: pages * PAGE_SIZE,
            is_cow: false,
            parent_id: None,
        })
    }

    /// Allocate a physical page for the VMO at the given byte `offset`
    /// and return its physical address.  Returns `Ok(None)` if the
    /// page is already committed.
    pub fn commit_page(&mut self, offset: usize) -> Result<Option<PhysAddr>> {
        if offset >= self.size {
            return Err(Status::InvalidArgs);
        }
        let page_idx = offset / PAGE_SIZE;
        let slot = self.page_slot(page_idx);
        unsafe {
            if (*slot).is_some() {
                return Ok(None);
            }
            let pa = phys::alloc_page()?;
            (*slot) = Some(pa);
            (*self.header()).committed += 1;
            Ok(Some(pa))
        }
    }

    /// Commit every page in the VMO.  Useful for small VMOs and for
    /// test paths where lazy allocation adds no value.
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
        let hs = header_size();

        // Allocate the first metadata page.
        let meta_pa = phys::alloc_page()?;
        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            for i in 0..PAGE_SIZE {
                core::ptr::write_volatile(base.add(i), 0);
            }

            let header = self.header();
            let new_header = base as *mut VmoHeader;
            (*new_header).magic = VMO_MAGIC;
            (*new_header).version = VMO_VERSION;
            (*new_header).capacity_pages = pages as u64;
            (*new_header).committed = (*header).committed;
            (*new_header).size_bytes = (*header).size_bytes;
            (*new_header).meta_page_count = mpn as u64;

            // Allocate remaining metadata pages and record PAs in the PA table.
            for i in 1..mpn {
                let extra_pa = phys::alloc_page()?;
                let table_off = PA_TABLE_BASE + (i - 1) * 8;
                core::ptr::write_volatile(
                    base.add(table_off) as *mut u64,
                    extra_pa.as_usize() as u64,
                );
                let eb = pa_to_kernel_va(extra_pa.as_usize()) as *mut u8;
                for j in 0..PAGE_SIZE {
                    core::ptr::write_volatile(eb.add(j), 0);
                }
            }
        }

        // Copy all page slots from self to the new VMO.
        for i in 0..pages {
            let old_pa = unsafe { *self.page_slot(i) };
            unsafe {
                *slot_ptr(meta_pa.as_usize(), mpn, i) = old_pa;
            }
        }

        Ok(Vmo {
            id: new_id,
            meta_pa,
            size: self.size,
            is_cow: true,
            parent_id: Some(self.id),
        })
    }

    /// Slice/slice-clone a sub-region of an existing VMO, creating a child VMO with offset/size boundaries
    /// that points to the parent's exact physical page list range without copying actual page payloads.
    pub fn create_child_slice(&self, new_id: u64, offset: usize, size: usize) -> Result<Self> {
        if offset & (PAGE_SIZE - 1) != 0 || size & (PAGE_SIZE - 1) != 0 {
            return Err(Status::InvalidArgs);
        }
        if offset + size > self.size {
            return Err(Status::InvalidArgs);
        }
        let pages = size / PAGE_SIZE;
        let start_page_idx = offset / PAGE_SIZE;
        let mpn = meta_pages_needed(pages);
        let hs = header_size();

        let meta_pa = phys::alloc_page()?;
        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            for i in 0..PAGE_SIZE {
                core::ptr::write_volatile(base.add(i), 0);
            }

            let new_header = base as *mut VmoHeader;
            (*new_header).magic = VMO_MAGIC;
            (*new_header).version = VMO_VERSION;
            (*new_header).capacity_pages = pages as u64;
            (*new_header).committed = pages as u64; // slices are committed since pages are borrowed
            (*new_header).size_bytes = size as u64;
            (*new_header).meta_page_count = mpn as u64;

            // Allocate remaining metadata pages and record PAs in the PA table.
            for i in 1..mpn {
                let extra_pa = phys::alloc_page()?;
                let table_off = PA_TABLE_BASE + (i - 1) * 8;
                core::ptr::write_volatile(
                    base.add(table_off) as *mut u64,
                    extra_pa.as_usize() as u64,
                );
                let eb = pa_to_kernel_va(extra_pa.as_usize()) as *mut u8;
                for j in 0..PAGE_SIZE {
                    core::ptr::write_volatile(eb.add(j), 0);
                }
            }

            for i in 0..pages {
                let old_slot = self.page_slot(start_page_idx + i);
                let new_slot = slot_ptr(meta_pa.as_usize(), mpn, i);
                *new_slot = *old_slot;
            }
        }

        Ok(Vmo {
            id: new_id,
            meta_pa,
            size,
            is_cow: false,
            parent_id: Some(self.id),
        })
    }

    pub fn make_cow(&mut self) {
        self.is_cow = true;
    }

    /// Read up to `buf.len()` bytes from the VMO at byte `offset`.
    /// Returns the number of bytes actually copied; reads past the
    /// VMO size return `Ok(0)`.  Reads from uncommitted pages return
    /// zeros (so callers don't have to special-case sparse VMOs).
    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        if offset >= self.size {
            return Ok(0);
        }
        let mut written = 0;
        let mut cur = offset;
        while written < buf.len() && cur < self.size {
            let page_idx = cur / PAGE_SIZE;
            let in_page = cur % PAGE_SIZE;
            let pa = unsafe { (*self.page_slot(page_idx)) };
            let available = match pa {
                Some(p) => {
                    let src = unsafe { pa_to_kernel_va(p.as_usize()) as *const u8 };
                    PAGE_SIZE - in_page
                }
                None => PAGE_SIZE - in_page,
            };
            let want = core::cmp::min(available, buf.len() - written);
            let end = core::cmp::min(cur + want, self.size);
            let real = end - cur;
            match pa {
                Some(p) => unsafe {
                    let src = pa_to_kernel_va(p.as_usize()).add(in_page);
                    core::ptr::copy_nonoverlapping(src as *const u8,
                                                   buf.as_mut_ptr().add(written),
                                                   real);
                },
                None => {
                    // zero-fill sparse region
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

    /// Write up to `buf.len()` bytes into the VMO at byte `offset`.
    /// Pages that the write touches are committed on demand.
    pub fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize> {
        if offset >= self.size {
            return Ok(0);
        }
        let mut written = 0;
        let mut cur = offset;
        while written < buf.len() && cur < self.size {
            let page_idx = cur / PAGE_SIZE;
            let in_page = cur % PAGE_SIZE;
            // Commit the page if necessary.
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
                core::ptr::copy_nonoverlapping(buf.as_ptr().add(written),
                                               dst as *mut u8,
                                               real);
                // Clean data cache to PoU (point of unification) for the written bytes!
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

    /// Return the physical address backing `offset`, if any.  Used by
    /// VMAR to build page-table entries.
    pub fn get_page_phys(&self, offset: usize) -> Option<PhysAddr> {
        if offset >= self.size {
            return None;
        }
        unsafe { (*self.page_slot(offset / PAGE_SIZE)) }
    }

    pub fn get_size(&self) -> usize { self.size }
    pub fn page_count(&self) -> usize { self.size / PAGE_SIZE }

    // --- private helpers ---

    fn header(&self) -> *mut VmoHeader {
        pa_to_kernel_va(self.meta_pa.as_usize()) as *mut VmoHeader
    }

    /// Return the number of metadata pages allocated for this VMO.
    fn meta_page_count(&self) -> usize {
        unsafe { (*self.header()).meta_page_count as usize }
    }

    /// Return a `*mut Option<PhysAddr>` for the slot that holds the
    /// physical page backing `page_idx`.
    pub fn page_slot(&self, page_idx: usize) -> *mut Option<PhysAddr> {
        unsafe { slot_ptr(self.meta_pa.as_usize(), self.meta_page_count(), page_idx) }
    }
}

fn round_up_to_pages(size: usize) -> usize {
    (size + PAGE_SIZE - 1) / PAGE_SIZE
}

/// Return a pointer to the slot for `page_idx` in the VMO whose first
/// metadata page is at `meta_pa` and which has `mpn` metadata pages
/// in total.  Independent of any `Vmo` instance.
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
        Vmo {
            id: self.id,
            meta_pa: self.meta_pa,
            size: self.size,
            is_cow: self.is_cow,
            parent_id: self.parent_id,
        }
    }
}
