use shared::status::{Result, Status};
use crate::mm::mmu::pa_to_kernel_va;
use crate::mm::phys;
use core::sync::atomic::{AtomicUsize, Ordering};

extern "C" {
    fn _kernel_start();
    fn _kernel_end();
}

pub(crate) static mut NEXT_FREE_PAGE: usize = 0;
pub(crate) static mut END_FREE_PAGE: usize = 0;
static mut FREE_PAGES_COUNT: usize = 0;
static mut TOTAL_PAGES_COUNT: usize = 0;
static mut MMU_ACTIVE: bool = false;

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
    if MMU_ACTIVE { pa_to_kernel_va(pa) as *mut u8 } else { pa as *mut u8 }
}

pub fn mark_mmu_active() {
    unsafe { MMU_ACTIVE = true; }
}

pub fn mmu_is_active() -> bool {
    unsafe { MMU_ACTIVE }
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
                unsafe {
                    NEXT_FREE_PAGE = current_page;
                }
                next_set = true;
            }
            free_count += 1;
        }

        current_page += 4096;
    }

    unsafe {
        END_FREE_PAGE = current_page;
        FREE_PAGES_COUNT = free_count;
        TOTAL_PAGES_COUNT = total_count;
    }
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
    unsafe {
        let pa = NEXT_FREE_PAGE;
        if pa == 0 || pa >= END_FREE_PAGE {
            return Err(Status::NoMemory);
        }
        NEXT_FREE_PAGE = pa + 4096;
        FREE_PAGES_COUNT = FREE_PAGES_COUNT.wrapping_sub(1);

        let ptr = page_ptr(pa);
        for i in 0..(4096 / 8) {
            core::ptr::write_volatile(ptr.add(i * 8) as *mut u64, 0);
        }

        Ok(PhysAddr::new(pa))
    }
}

pub fn free_page(addr: PhysAddr) -> Status {
    unsafe {
        PAGE_CACHE.add_page(addr, false);
    }
    Status::Ok
}

pub fn get_free_pages_count() -> usize {
    unsafe { FREE_PAGES_COUNT }
}

pub fn get_total_pages_count() -> usize {
    unsafe { TOTAL_PAGES_COUNT }
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
