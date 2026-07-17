use shared::status::{Result, Status};
use crate::arch::mmu_facade::pa_to_kernel_va;
use crate::arch::aarch64::phys;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

extern "C" {
    fn _kernel_start();
    fn _kernel_end();
}

pub(crate) static NEXT_FREE_PAGE: AtomicUsize = AtomicUsize::new(0);
pub(crate) static END_FREE_PAGE: AtomicUsize = AtomicUsize::new(0);
static FREE_PAGES_COUNT: AtomicUsize = AtomicUsize::new(0);
static TOTAL_PAGES_COUNT: AtomicUsize = AtomicUsize::new(0);
static MMU_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Dynamic WATCH physical address.  Set during PID 1's FINAL-CANARY
/// to the actual L3 page PA for that boot.  All WATCH statements that
/// formerly used hardcoded addresses are now compared against this
/// value, making the instrumentation reliable across non-deterministic
/// cursor progress.
pub static WATCH_PA: AtomicUsize = AtomicUsize::new(0);

// ── Live Page-Table Page Tracker ──────────────────────────────────
// Maintains a small set of physical addresses that are currently in
// use as page-table pages (tracked by any process's PageTableTree).
// Before every alloc_page we verify the candidate PA is NOT in this
// set, catching use-after-free / double-alloc of PT pages even when
// the page-tag pre-check doesn't fire (e.g. the page was freed and
// re-tagged Free before alloc_page reallocates it).

const MAX_LIVE_PT: usize = 64;
static LIVE_PT_PAGES: [AtomicUsize; MAX_LIVE_PT] = [const { AtomicUsize::new(0) }; MAX_LIVE_PT];

pub fn register_pt_page(pa: usize) {
    if pa == 0 { return; }
    for slot in &LIVE_PT_PAGES {
        let prev = slot.load(Ordering::Relaxed);
        if prev == pa { return; } // already registered
        if prev == 0 {
            if slot.compare_exchange(0, pa, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                return;
            }
        }
    }
    // No room — not fatal, just means we can't track all PT pages.
    crate::log_warn!("PHYS", "register_pt_page: tracker full (pa={:#x})", pa);
}

pub fn unregister_pt_page(pa: usize) {
    if pa == 0 { return; }
    for slot in &LIVE_PT_PAGES {
        let prev = slot.load(Ordering::Relaxed);
        if prev == pa {
            let _ = slot.compare_exchange(pa, 0, Ordering::AcqRel, Ordering::Relaxed);
            return;
        }
    }
}

fn is_live_pt_page(pa: usize) -> bool {
    if pa == 0 { return false; }
    for slot in &LIVE_PT_PAGES {
        if slot.load(Ordering::Relaxed) == pa {
            return true;
        }
    }
    false
}

// ── Tagged page allocator ────────────────────────────────────────────
// Every physical 4 KiB frame carries a one-byte tag so we can detect
// use-after-free, double-free, and type-confusion bugs that were
// previously silent (PHYS-GUARD was a hardcoded PA list).

const RAM_BASE: usize = 0x4000_0000;
const RAM_SIZE: usize = 0x2000_0000;
const MAX_PAGES: usize = RAM_SIZE / 4096; // 131072

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageTag {
    Free = 0,
    PageTable = 1,
    VmoData = 2,
    VmarMeta = 3,
    KernelStack = 4,
    KernelHeap = 5,
}

/// One-byte tag per page frame.  Indexed by `pa_to_pfn(pa)`.
/// Init to `Free` (0) — the `.bss` linker zeroes the whole array.
static mut PAGE_TAGS: [u8; MAX_PAGES] = [0u8; MAX_PAGES];

#[inline(always)]
fn pa_to_pfn(pa: usize) -> usize {
    (pa - RAM_BASE) >> 12
}

#[inline(always)]
fn pa_valid(pa: usize) -> bool {
    pa >= RAM_BASE && pa < RAM_BASE + RAM_SIZE
}

/// Read the tag of a physical page without modifying it.
/// Returns `None` if the address is outside the RAM range.
pub fn page_tag(pa: usize) -> Option<PageTag> {
    if !pa_valid(pa) { return None; }
    let tag = unsafe { PAGE_TAGS[pa_to_pfn(pa)] };
    match tag {
        0 => Some(PageTag::Free),
        1 => Some(PageTag::PageTable),
        2 => Some(PageTag::VmoData),
        3 => Some(PageTag::VmarMeta),
        4 => Some(PageTag::KernelStack),
        5 => Some(PageTag::KernelHeap),
        _ => None,
    }
}

pub const PAGE_CACHE_SIZE: usize = 64;

pub struct PageCacheEntry {
    pa: PhysAddr,
    age: usize,
    dirty: bool,
}

pub struct PageCache {
    entries: [Option<PageCacheEntry>; PAGE_CACHE_SIZE],
    count: usize,
}

impl PageCache {
    pub const fn new() -> Self {
        PageCache {
            entries: [const { None }; PAGE_CACHE_SIZE],
            count: 0,
        }
    }

    pub fn init(&mut self) {
        *self = Self::new();
    }

    fn find_evictable(&self) -> Option<(usize, PhysAddr)> {
        let mut oldest_idx = None;
        let mut oldest_age = usize::MAX;

        for i in 0..PAGE_CACHE_SIZE {
            if let Some(ref entry) = self.entries[i] {
                if !entry.dirty && entry.age < oldest_age {
                    oldest_age = entry.age;
                    oldest_idx = Some(i);
                }
            }
        }

        if let Some(idx) = oldest_idx {
            if let Some(entry) = &self.entries[idx] {
                return Some((idx, entry.pa));
            }
        }

        for i in 0..PAGE_CACHE_SIZE {
            if let Some(ref entry) = self.entries[i] {
                if entry.dirty {
                    return Some((i, entry.pa));
                }
            }
        }

        None
    }

    pub fn add_page(&mut self, pa: PhysAddr, dirty: bool) {
        if self.count >= PAGE_CACHE_SIZE {
            if let Some((idx, _evicted_pa)) = self.find_evictable() {
                // Drop the evicted entry — the page is already tagged Free
                // and the cursor-based allocator doesn't recycle.  Don't
                // call phys::free_page here or we'd recurse and double-free.
                self.entries[idx] = None;
                self.count -= 1;
            }
        }

        for i in 0..PAGE_CACHE_SIZE {
            if self.entries[i].is_none() {
                self.entries[i] = Some(PageCacheEntry {
                    pa,
                    age: 0,
                    dirty,
                });
                self.count += 1;
                return;
            }
        }
    }

    pub fn mark_dirty(&mut self, pa: PhysAddr) {
        for entry in &mut self.entries {
            if let Some(ref mut e) = entry {
                if e.pa == pa {
                    e.dirty = true;
                    return;
                }
            }
        }
    }

    pub fn tick(&mut self) {
        for entry in &mut self.entries {
            if let Some(ref mut e) = entry {
                e.age += 1;
            }
        }
    }

    pub fn sync(&mut self) -> Result<usize> {
        let mut synced = 0;
        for entry in &mut self.entries {
            if let Some(ref mut e) = entry {
                if e.dirty {
                    synced += 1;
                    e.dirty = false;
                }
            }
        }
        Ok(synced)
    }

    pub fn drain(&mut self) -> usize {
        let mut freed = 0;
        for entry in self.entries.iter_mut() {
            if let Some(_e) = entry.take() {
                // Page is already tagged Free; just count it.
                freed += 1;
            }
        }
        self.count = 0;
        freed
    }
}

static mut PAGE_CACHE: PageCache = PageCache::new();

#[inline(always)]
 unsafe fn page_ptr(pa: usize) -> *mut u8 {
    if pa == 0x40254000 {
        crate::log_debug!("WATCH", "phys::page_ptr called for PA=0x40254000");
    }
    if pa == 0x40255000 {
        crate::log_debug!("WATCH", "phys::page_ptr called for PA=0x40255000");
    }
    let watched = WATCH_PA.load(Ordering::Relaxed);
    if watched != 0 && pa == watched {
        crate::log_debug!("WATCH", "phys::page_ptr called for dynamic WATCH PA={:#x}", pa);
    }
    if MMU_ACTIVE.load(Ordering::Acquire) { pa_to_kernel_va(pa) as *mut u8 } else { pa as *mut u8 }
}

pub fn mark_mmu_active() {
    MMU_ACTIVE.store(true, Ordering::Release);
}

pub fn mmu_is_active() -> bool {
    MMU_ACTIVE.load(Ordering::Acquire)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(usize);

impl PhysAddr {
    pub const fn new(addr: usize) -> Self {
        PhysAddr(addr)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }
}

impl From<usize> for PhysAddr {
    fn from(addr: usize) -> Self {
        PhysAddr(addr)
    }
}

pub fn init(ram_base: usize, ram_size: usize) {
    let _kernel_start_addr = _kernel_start as usize;
    let kernel_end_addr = _kernel_end as usize;

    let kernel_end_page = (kernel_end_addr + 4095) & !4095;
    let ram_end = ram_base + ram_size;

    let dtb_addr = unsafe { crate::DTB_POINTER as usize };
    let (dtb_start_page, dtb_end_page) = if dtb_addr != 0 {
        let size_be = unsafe { core::ptr::read_volatile((dtb_addr + 4) as *const u32) };
        let dtb_size = u32::from_be(size_be) as usize;
        let start = dtb_addr & !4095;
        let end = (dtb_addr + dtb_size + 4095) & !4095;
        (start, end)
    } else {
        (0, 0)
    };

    let mut free_count = 0;
    let mut total_count = 0;
    let mut next_set = false;

    let mut current_page = kernel_end_page;
    while current_page + 4096 <= ram_end {
        total_count += 1;

        if !is_page_reserved(current_page, kernel_end_page, dtb_start_page, dtb_end_page) {
            if !next_set {
                NEXT_FREE_PAGE.store(current_page, Ordering::Release);
                next_set = true;
            }
            free_count += 1;
        }

        current_page += 4096;
    }

    END_FREE_PAGE.store(current_page, Ordering::Release);
    FREE_PAGES_COUNT.store(free_count, Ordering::Release);
    TOTAL_PAGES_COUNT.store(total_count, Ordering::Release);
}

fn is_page_reserved(page_addr: usize, kernel_end_page: usize, dtb_start_page: usize, dtb_end_page: usize) -> bool {
    if page_addr < kernel_end_page {
        return true;
    }
    if dtb_start_page != 0 && page_addr >= dtb_start_page && page_addr < dtb_end_page {
        return true;
    }
    false
}

pub fn alloc_page(tag: PageTag) -> Result<PhysAddr> {
    loop {
        let pa = NEXT_FREE_PAGE.load(Ordering::Acquire);
        let end = END_FREE_PAGE.load(Ordering::Acquire);
        if pa == 0 || pa >= end {
            return Err(Status::NoMemory);
        }

        if pa >= 0x40251000 && pa < 0x40259000 {
            crate::log_debug!("WATCH", "alloc_page cursor in range PA={:#x} tag={:?}", pa, tag);
        }
        let watched = WATCH_PA.load(Ordering::Relaxed);
        if watched != 0 && pa >= watched && pa < watched + 4096 {
            crate::log_debug!("WATCH", "alloc_page cursor at dynamic WATCH PA={:#x} tag={:?}", pa, tag);
        }

        // [FIX] Tag guard BEFORE compare_exchange: if the page is already
        // allocated (not Free), advance the cursor past it and retry.
        let pfn = pa_to_pfn(pa);
        let old_tag_pre = unsafe { PAGE_TAGS[pfn] };
            if old_tag_pre != PageTag::Free as u8 {
                crate::log_error!("PHYS", "alloc_page SKIP PA={:#x} tag={} (expected Free) caller={:?} cursor={:#x}",
                    pa, old_tag_pre, tag, NEXT_FREE_PAGE.load(Ordering::Relaxed));
                let next = pa + 4096;
                let _ = NEXT_FREE_PAGE.compare_exchange(pa, next, Ordering::AcqRel, Ordering::Acquire);
                continue;
            }

            // LIVE-PT guard: panic if we're about to allocate a page
            // that is still registered as a live page-table page.
            if is_live_pt_page(pa) {
                panic!(
                    "PHYS: alloc_page PA={:#x} tag={:?} but still in LIVE_PT_PAGES tracker!",
                    pa, tag,
                );
            }

        let next = pa + 4096;
        if NEXT_FREE_PAGE
            .compare_exchange(pa, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            FREE_PAGES_COUNT.fetch_sub(1, Ordering::AcqRel);

            if pa == 0x40254000 {
                crate::log_debug!("WATCH", "phys::alloc_page called for PA=0x40254000 tag={:?}", tag);
            }
            if pa == 0x40255000 {
                crate::log_debug!("WATCH", "phys::alloc_page called for PA=0x40255000 tag={:?}", tag);
            }
            let watched = WATCH_PA.load(Ordering::Relaxed);
            if watched != 0 && pa == watched {
                crate::log_debug!("WATCH", "phys::alloc_page called for dynamic WATCH PA={:#x} tag={:?}", pa, tag);
            }
            if watched != 0 && pa >= watched && pa < watched + 4096 {
                crate::log_debug!("WATCH", "phys::alloc_page called INSIDE dynamic WATCH page PA={:#x} tag={:?}", pa, tag);
            }
            let old_tag = unsafe { PAGE_TAGS[pfn] };
            if old_tag != PageTag::Free as u8 {
                panic!(
                    "PHYS: PA={:#x} tag={} (expected Free) caller={:?}",
                    pa, old_tag, tag,
                );
            }
            unsafe { PAGE_TAGS[pfn] = tag as u8; }

            #[cfg(debug_assertions)]
            crate::log_info!("PHYS", "alloc_page -> PA={:#x} tag={:?}", pa, tag);

            let ptr = unsafe { page_ptr(pa) };
            for i in 0..(4096 / 8) {
                unsafe {
                    core::ptr::write_volatile(ptr.add(i * 8) as *mut u64, 0);
                }
            }

            // Clean the entire page from the data cache so no stale
            // zero-dirty cache lines survive to overwrite valid PTEs
            // written later (the L3 corruption bug).  Only needed when
            // the MMU is active and the page will be used as a page
            // table, but unconditional is harmless and cheap.
            #[cfg(target_arch = "aarch64")]
            if MMU_ACTIVE.load(Ordering::Acquire) {
                let base = ptr as usize;
                let step = 64usize;
                let end = base + 4096;
                let mut cur = base;
                while cur < end {
                    unsafe {
                        core::arch::asm!("dc civac, {0}", in(reg) cur, options(nomem, nostack));
                    }
                    cur += step;
                }
                unsafe {
                    core::arch::asm!("dsb ish", options(nomem, nostack));
                }
            }

            return Ok(PhysAddr::new(pa));
        }
        core::hint::spin_loop();
    }
}

pub fn free_page(addr: PhysAddr) -> Status {
    let pa = addr.as_usize();
    if pa == 0x40254000 {
        crate::log_debug!("WATCH", "phys::free_page called for PA=0x40254000 (stacktrace follows)");
    }
    if pa == 0x40255000 {
        crate::log_debug!("WATCH", "phys::free_page called for PA=0x40255000 (stacktrace follows)");
    }
    let watched = WATCH_PA.load(Ordering::Relaxed);
    if watched != 0 && pa == watched {
        crate::log_debug!("WATCH", "phys::free_page called for dynamic WATCH PA={:#x} (stacktrace follows)", pa);
    }
    if watched != 0 && pa >= watched && pa < watched + 4096 {
        crate::log_debug!("WATCH", "phys::free_page called INSIDE dynamic WATCH page PA={:#x}", pa);
    }
    let pfn = pa_to_pfn(pa);
    let old_tag = unsafe { PAGE_TAGS[pfn] };
    if old_tag == PageTag::Free as u8 {
        panic!("PHYS: double-free PA={:#x}", pa);
    }
    // Unregister from page-table tracker if this was a PT page.
    if old_tag == PageTag::PageTable as u8 {
        unregister_pt_page(pa);
    }
    unsafe { PAGE_TAGS[pfn] = PageTag::Free as u8; }
    unsafe { PAGE_CACHE.add_page(addr, false); }
    Status::Ok
}

// ── Typed allocation wrappers ────────────────────────────────────────
#[inline]
pub fn alloc_pt_page() -> Result<PhysAddr> { alloc_page(PageTag::PageTable) }

#[inline]
pub fn alloc_vmo_data() -> Result<PhysAddr> { alloc_page(PageTag::VmoData) }

#[inline]
pub fn alloc_vmo_meta() -> Result<PhysAddr> { alloc_page(PageTag::VmoData) }

#[inline]
pub fn alloc_vmar_meta() -> Result<PhysAddr> { alloc_page(PageTag::VmarMeta) }

#[inline]
pub fn alloc_kstack_page() -> Result<PhysAddr> { alloc_page(PageTag::KernelStack) }

#[inline]
pub fn alloc_kheap_page() -> Result<PhysAddr> { alloc_page(PageTag::KernelHeap) }

pub fn get_free_pages_count() -> usize {
    FREE_PAGES_COUNT.load(Ordering::Acquire)
}

pub fn get_total_pages_count() -> usize {
    TOTAL_PAGES_COUNT.load(Ordering::Acquire)
}

pub fn reclaim_page() -> Option<PhysAddr> {
    unsafe {
        if let Some((_, pa)) = PAGE_CACHE.find_evictable() {
            PAGE_CACHE.add_page(pa, false);
            Some(pa)
        } else {
            None
        }
    }
}

pub fn tick_page_cache() {
    unsafe { PAGE_CACHE.tick(); }
}

pub fn sync_page_cache() -> Result<usize> {
    unsafe { PAGE_CACHE.sync() }
}
