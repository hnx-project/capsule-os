use core::alloc::{Layout, GlobalAlloc};
use core::ptr;

pub struct BumpAllocator {
    head: core::sync::atomic::AtomicPtr<u8>,
    size: usize,
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let mut current = self.head.load(core::sync::atomic::Ordering::Relaxed);
        let mut aligned = (current as usize + align - 1) & !(align - 1);
        let end = aligned + size;
        if end > current as usize + self.size {
            ptr::null_mut()
        } else {
            self.head.store(end as *mut u8, core::sync::atomic::Ordering::Relaxed);
            aligned as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {
    head: core::sync::atomic::AtomicPtr::new(ptr::null_mut()),
    size: 0,
};

pub fn init(start: *mut u8, size: usize) {
    ALLOCATOR.head.store(start, core::sync::atomic::Ordering::Relaxed);
}
