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
        let ci = match class_index(layout.size(), layout.align()) {
            Some(i) => i,
            None => {
                // Large allocation (> 2048 bytes): Allocate contiguous page frames directly
                let size = layout.size();
                let needed_bytes = size + 8; // 8-byte header to store page count
                let pages = (needed_bytes + PAGE_SIZE - 1) / PAGE_SIZE;

                let mut start_pa = 0;
                for i in 0..pages {
                    match phys::alloc_kheap_page() {
                        Ok(p) => {
                            if i == 0 {
                                start_pa = p.as_usize();
                            } else if p.as_usize() != start_pa + i * PAGE_SIZE {
                                // Non-contiguous fallback: free allocated pages and return null
                                unsafe {
                                    for j in 0..i {
                                        phys::free_page(PhysAddr::new(start_pa + j * PAGE_SIZE));
                                    }
                                }
                                return ptr::null_mut();
                            }
                        }
                        Err(_) => {
                            if i > 0 {
                                unsafe {
                                    for j in 0..i {
                                        phys::free_page(PhysAddr::new(start_pa + j * PAGE_SIZE));
                                    }
                                }
                            }
                            return ptr::null_mut();
                        }
                    }
                }

                let kva = pa_to_kernel_va(start_pa) as *mut u8;
                unsafe {
                    // Write page count in first 8 bytes
                    ptr::write(kva as *mut usize, pages);
                    // Return pointer after header
                    return kva.add(8);
                }
            }
        };

        if layout.align() > 64 {
            return ptr::null_mut();
        }

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
            None => {
                // Large allocation: retrieve page count from header and free the pages
                unsafe {
                    let kva = ptr.sub(8);
                    let pages = ptr::read(kva as *const usize);
                    let start_pa = kva as usize - 0xffff800000000000;
                    for i in 0..pages {
                        phys::free_page(PhysAddr::new(start_pa + i * PAGE_SIZE));
                    }
                }
                return;
            }
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
