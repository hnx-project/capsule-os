use shared::status::Status;

use crate::mm::mmu::pa_to_kernel_va;

extern "C" {
    fn _kernel_start();
    fn _kernel_end();
}

pub(crate) static mut NEXT_FREE_PAGE: usize = 0;
pub(crate) static mut END_FREE_PAGE: usize = 0;
static mut FREE_PAGES_COUNT: usize = 0;
static mut TOTAL_PAGES_COUNT: usize = 0;
static mut MMU_ACTIVE: bool = false;

#[inline(always)]
unsafe fn page_ptr(pa: usize) -> *mut u8 {
    if MMU_ACTIVE { pa_to_kernel_va(pa) as *mut u8 } else { pa as *mut u8 }
}

pub fn mark_mmu_active() {
    unsafe { MMU_ACTIVE = true; }
}

/// Returns true once `mark_mmu_active` has been called.  Used by the
/// page-table helpers in `arch::mmu` to decide whether to access
/// page-table pages via `pa` (pre-MMU) or `pa_to_kernel_va(pa)`
/// (post-MMU).
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

pub fn init(ram_base: usize, ram_size: usize) {
    let _kernel_start_addr = _kernel_start as usize;
    let kernel_end_addr = _kernel_end as usize;

    let kernel_end_page = (kernel_end_addr + 4095) & !4095;
    let ram_end = ram_base + ram_size;

    // Get DTB range
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

pub fn alloc_page() -> Result<PhysAddr, Status> {
    unsafe {
        let pa = NEXT_FREE_PAGE;
        if pa == 0 || pa >= END_FREE_PAGE {
            return Err(Status::NoMemory);
        }
        NEXT_FREE_PAGE = pa + 4096;
        FREE_PAGES_COUNT = FREE_PAGES_COUNT.wrapping_sub(1);

        // Zero the page
        let ptr = page_ptr(pa);
        for i in 0..(4096 / 8) {
            core::ptr::write_volatile(ptr.add(i * 8) as *mut u64, 0);
        }

        Ok(PhysAddr::new(pa))
    }
}

pub fn free_page(_addr: PhysAddr) -> Status {
    // Bump allocator does not support freeing individual pages yet.
    Status::Ok
}

pub fn get_free_pages_count() -> usize {
    unsafe { FREE_PAGES_COUNT }
}

pub fn get_total_pages_count() -> usize {
    unsafe { TOTAL_PAGES_COUNT }
}
