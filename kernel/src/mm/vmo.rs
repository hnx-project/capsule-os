//! Virtual Memory Object (VMO).
//!
//! A VMO is a kernel-managed collection of physical pages that can be mapped
//! into any VMAR.  This is a stripped-down Zircon-style VMO; the only kind
//! of backing store we support is anonymous (contiguous-in-`PhysAddr`-index
//! space, not necessarily contiguous in physical address space).
//!
//! ## On-disk layout of the metadata
//!
//! Each VMO owns one metadata page allocated from the physical page
//! allocator.  The first 64 bits of the page store `magic`/`version`
//! and `capacity`; the rest of the page is a `[Option<PhysAddr>; N]`
//! table that records which offsets have a committed physical page.
//!
//! `N` is fixed at compile time to keep the type `no_std` and
//! allocation-free for the metadata itself.  By default `N = 512`,
//! so a single VMO can address at most 512 × 4 KiB = 2 MiB.  When a
//! future release needs bigger VMOs, swap in a chained metadata
//! design (the same scheme FTLs use).

use core::sync::atomic::{AtomicUsize, Ordering};

use shared::status::{Result, Status};

use core::ops::Add;

use crate::mm::mmu::pa_to_kernel_va;
use crate::mm::phys::{self, PhysAddr};

const VMO_MAGIC: u64 = 0x564D_4F4D_4147_4341; // "VMOMAGCA" (visible in hex dumps)
const VMO_VERSION: u64 = 1;
const PAGE_SIZE: usize = 4096;
/// Maximum number of pages a single VMO can address.
pub const VMO_MAX_PAGES: usize = 512;

static VMO_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Header stored at the start of the metadata page.
#[repr(C)]
struct VmoHeader {
    magic: u64,
    version: u64,
    capacity_pages: u64,
    /// Number of pages currently committed (allocated physical pages).
    committed: u64,
    /// Size in bytes (capacity * 4096).
    size_bytes: u64,
}

/// The user-facing VMO handle.
#[derive(Debug)]
pub struct Vmo {
    pub id: u64,
    meta_pa: PhysAddr,
    size: usize,
    pub is_cow: bool,
    pub parent_id: Option<u64>,
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
        let meta_pa = phys::alloc_page()?;

        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            // Zero the page so all "uncommitted" slots are obvious.
            for i in 0..PAGE_SIZE {
                core::ptr::write_volatile(base.add(i), 0);
            }
            let header = base as *mut VmoHeader;
            (*header).magic = VMO_MAGIC;
            (*header).version = VMO_VERSION;
            (*header).capacity_pages = pages as u64;
            (*header).committed = 0;
            (*header).size_bytes = (pages * PAGE_SIZE) as u64;
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
        let meta_pa = phys::alloc_page()?;
        let pages = self.size / PAGE_SIZE;

        unsafe {
            let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
            for i in 0..PAGE_SIZE {
                core::ptr::write_volatile(base.add(i), 0);
            }

            let header = self.header();
            let new_header = pa_to_kernel_va(meta_pa.as_usize()) as *mut VmoHeader;
            (*new_header).magic = VMO_MAGIC;
            (*new_header).version = VMO_VERSION;
            (*new_header).capacity_pages = pages as u64;
            (*new_header).committed = (*header).committed;
            (*new_header).size_bytes = (*header).size_bytes;

            for i in 0..pages {
                let old_slot = self.page_slot(i);
                let new_slot = {
                    let base = pa_to_kernel_va(meta_pa.as_usize()) as *mut u8;
                    let offset = core::mem::size_of::<VmoHeader>()
                        + i * core::mem::size_of::<Option<PhysAddr>>();
                    base.add(offset) as *mut Option<PhysAddr>
                };
                (*new_slot) = (*old_slot);
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

    /// Return a `*mut Option<PhysAddr>` for the slot that holds the
    /// physical page backing `page_idx`.
    fn page_slot(&self, page_idx: usize) -> *mut Option<PhysAddr> {
        debug_assert!(page_idx < VMO_MAX_PAGES);
        unsafe {
            let base = pa_to_kernel_va(self.meta_pa.as_usize()) as *mut u8;
            let offset = core::mem::size_of::<VmoHeader>()
                + page_idx * core::mem::size_of::<Option<PhysAddr>>();
            base.add(offset) as *mut Option<PhysAddr>
        }
    }
}

fn round_up_to_pages(size: usize) -> usize {
    (size + PAGE_SIZE - 1) / PAGE_SIZE
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
