use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use core::ptr;

use crate::arch::aarch64::phys::{self, PhysAddr};
use crate::arch::mmu_facade::pa_to_kernel_va;

const CLASS_SIZES: [usize; 7] = [32, 64, 128, 256, 512, 1024, 2048];
const PAGE_SIZE: usize = 4096;
const PAGE_HEADER: usize = 8;

fn class_index(size: usize, align: usize) -> Option<usize> {
    let need = size.max(align);
    for (i, &cs) in CLASS_SIZES.iter().enumerate() {
        if need <= cs {
            return Some(i);
        }
    }
    None
}

fn blocks_per_page(block_size: usize) -> usize {
    (PAGE_SIZE - PAGE_HEADER) / block_size
}

pub struct SlabAllocator {
    heads: [AtomicUsize; CLASS_SIZES.len()],
}

unsafe impl Send for SlabAllocator {}
unsafe impl Sync for SlabAllocator {}

impl SlabAllocator {
    pub const fn new() -> Self {
        SlabAllocator {
            heads: [
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ],
        }
    }

    fn refill(&self, ci: usize) -> *mut u8 {
        let block_size = CLASS_SIZES[ci];
        let n = blocks_per_page(block_size);
        if n == 0 {
            return ptr::null_mut();
        }

        let pa = match phys::alloc_kheap_page() {
            Ok(p) => p,
            Err(_) => return ptr::null_mut(),
        };
        let kva = unsafe { pa_to_kernel_va(pa.as_usize()) as *mut u8 };

        // First 8 bytes of the page: pointer to previous head (for tracking)
        let prev = self.heads[ci].load(Ordering::Acquire);
        unsafe {
            ptr::write(kva as *mut usize, prev);
        }

        // Build free list within the page
        let data_start = unsafe { kva.add(PAGE_HEADER) };
        let last = n - 1;
        for i in 0..last {
            let block = unsafe { data_start.add(i * block_size) };
            let next = unsafe { data_start.add((i + 1) * block_size) };
            unsafe {
                ptr::write(block as *mut *mut u8, next);
            }
        }
        // Last block has null next
        let last_block = unsafe { data_start.add(last * block_size) };
        unsafe {
            ptr::write(last_block as *mut *mut u8, ptr::null_mut());
        }

        // Set head to first block
        self.heads[ci].store(data_start as usize, Ordering::Release);
        data_start
    }

    fn allocate(&self, layout: &Layout) -> *mut u8 {
        if layout.align() > 64 {
            return ptr::null_mut();
        }
        let ci = match class_index(layout.size(), layout.align()) {
            Some(i) => i,
            None => return ptr::null_mut(),
        };

        loop {
            let head = self.heads[ci].load(Ordering::Acquire);
            if head != 0 {
                let next_free = unsafe { ptr::read(head as *const *mut u8) };
                if self.heads[ci].compare_exchange(
                    head, next_free as usize,
                    Ordering::AcqRel, Ordering::Acquire,
                ).is_ok() {
                    return head as *mut u8;
                }
            } else {
                self.refill(ci);
                if self.heads[ci].load(Ordering::Acquire) == 0 {
                    return ptr::null_mut();
                }
            }
        }
    }

    fn deallocate(&self, ptr: *mut u8, layout: &Layout) {
        let ci = match class_index(layout.size(), layout.align()) {
            Some(i) => i,
            None => return,
        };

        loop {
            let head = self.heads[ci].load(Ordering::Acquire);
            unsafe {
                ptr::write(ptr as *mut *mut u8, head as *mut u8);
            }
            if self.heads[ci].compare_exchange(
                head, ptr as usize,
                Ordering::AcqRel, Ordering::Acquire,
            ).is_ok() {
                return;
            }
        }
    }
}

unsafe impl GlobalAlloc for SlabAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(&layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.deallocate(ptr, &layout)
    }
}

#[global_allocator]
static KERNEL_ALLOC: SlabAllocator = SlabAllocator::new();
