use shared::status::{Result, Status};
use crate::mm::mmu::pa_to_kernel_va;
use crate::mm::phys;
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
            if let Some((idx, evicted_pa)) = self.find_evictable() {
                self.entries[idx] = None;
                self.count -= 1;
                let _ = phys::free_page(evicted_pa);
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
            if let Some(e) = entry.take() {
                let _ = phys::free_page(e.pa);
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

pub fn alloc_page() -> Result<PhysAddr> {
    // K3 (KERNEL_HEALTH): race-free under SMP / IRQ-嵌套.  The
    // loop is bounded by `END_FREE_PAGE` (set during `init` and
    // read-only after that point); each iteration atomically
    // bumps NEXT_FREE_PAGE and decrements the free-page counter.
    // If two callers race, one of them will observe an out-of-
    // range page and bail with `Err(Status::NoMemory)` — the
    // caller is expected to retry or surface the error.
    loop {
        let pa = NEXT_FREE_PAGE.load(Ordering::Acquire);
        let end = END_FREE_PAGE.load(Ordering::Acquire);
        if pa == 0 || pa >= end {
            return Err(Status::NoMemory);
        }
        let next = pa + 4096;
        if NEXT_FREE_PAGE
            .compare_exchange(pa, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            FREE_PAGES_COUNT.fetch_sub(1, Ordering::AcqRel);

            let ptr = unsafe { page_ptr(pa) };
            for i in 0..(4096 / 8) {
                unsafe {
                    core::ptr::write_volatile(ptr.add(i * 8) as *mut u64, 0);
                }
            }

            return Ok(PhysAddr::new(pa));
        }
        // CAS failed — another CPU advanced the cursor; retry.
        core::hint::spin_loop();
    }
}

pub fn free_page(addr: PhysAddr) -> Status {
    unsafe {
        PAGE_CACHE.add_page(addr, false);
    }
    Status::Ok
}

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
