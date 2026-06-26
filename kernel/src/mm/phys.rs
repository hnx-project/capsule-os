use shared::status::Status;

extern "C" {
    fn _kernel_start();
    fn _kernel_end();
}

static mut FREE_LIST_HEAD: usize = 0;
static mut FREE_PAGES_COUNT: usize = 0;
static mut TOTAL_PAGES_COUNT: usize = 0;

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

    // Get DTB range dynamically from its header
    let dtb_addr = unsafe { crate::DTB_POINTER as usize };
    let (dtb_start_page, dtb_end_page) = if dtb_addr != 0 {
        // Read totalsize field at offset 4 of FDT header (big endian)
        let size_be = unsafe { core::ptr::read_volatile((dtb_addr + 4) as *const u32) };
        let dtb_size = u32::from_be(size_be) as usize;
        let start = dtb_addr & !4095;
        let end = (dtb_addr + dtb_size + 4095) & !4095;
        (start, end)
    } else {
        (0, 0)
    };

    let mut head = 0;
    let mut free_count = 0;
    let mut total_count = 0;

    let mut current_page = kernel_end_page;
    let ram_end = ram_base + ram_size;

    while current_page + 4096 <= ram_end {
        total_count += 1;

        if !is_page_reserved(current_page, kernel_end_page, dtb_start_page, dtb_end_page) {
            // Write the current head address into the first 8 bytes of the new free page
            unsafe {
                core::ptr::write_volatile(current_page as *mut usize, head);
            }
            head = current_page;
            free_count += 1;
        }

        current_page += 4096;
    }

    unsafe {
        FREE_LIST_HEAD = head;
        FREE_PAGES_COUNT = free_count;
        TOTAL_PAGES_COUNT = total_count;
    }
}

fn is_page_reserved(page_addr: usize, kernel_end_page: usize, dtb_start_page: usize, dtb_end_page: usize) -> bool {
    // Check if within bootloader/kernel region (ram_base .. kernel_end_page)
    if page_addr < kernel_end_page {
        return true;
    }
    // Check if within DTB region
    if dtb_start_page != 0 && page_addr >= dtb_start_page && page_addr < dtb_end_page {
        return true;
    }
    false
}

pub fn alloc_page() -> Result<PhysAddr, Status> {
    unsafe {
        let head = FREE_LIST_HEAD;
        if head == 0 {
            return Err(Status::NoMemory);
        }

        // Pop the first page from the free list
        let next_head = core::ptr::read_volatile(head as *const usize);
        FREE_LIST_HEAD = next_head;
        FREE_PAGES_COUNT -= 1;

        // Zero-initialize the allocated page for safety and deterministic state
        let page_ptr = head as *mut u64;
        for i in 0..(4096 / 8) {
            core::ptr::write_volatile(page_ptr.add(i), 0);
        }

        Ok(PhysAddr::new(head))
    }
}

pub fn free_page(addr: PhysAddr) -> Status {
    let page_addr = addr.as_usize();
    if page_addr % 4096 != 0 {
        return Status::InvalidArgs;
    }

    unsafe {
        // Push the page back to the free list
        core::ptr::write_volatile(page_addr as *mut usize, FREE_LIST_HEAD);
        FREE_LIST_HEAD = page_addr;
        FREE_PAGES_COUNT += 1;
    }

    Status::Ok
}

pub fn get_free_pages_count() -> usize {
    unsafe { FREE_PAGES_COUNT }
}

pub fn get_total_pages_count() -> usize {
    unsafe { TOTAL_PAGES_COUNT }
}
