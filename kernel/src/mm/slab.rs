use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::mm::phys;

pub const SLAB_CLASS_COUNT: usize = 8;
pub const SLAB_SIZE: usize = 4096;

pub const SLAB_SIZES: [usize; SLAB_CLASS_COUNT] = [
    8, 16, 32, 64, 128, 256, 512, 1024,
];

pub struct SlabClass {
    size: usize,
    free_list: [Option<usize>; 64],
    free_count: usize,
}

impl SlabClass {
    const fn new(size: usize) -> Self {
        SlabClass {
            size,
            free_list: [const { None }; 64],
            free_count: 0,
        }
    }

    fn allocate(&mut self) -> Result<usize> {
        if self.free_count > 0 {
            self.free_count -= 1;
            let addr = self.free_list[self.free_count].take();
            return addr.ok_or(Status::NoMemory);
        }

        let page = phys::alloc_page()?;
        let base = page.as_usize();

        let objects_per_page = SLAB_SIZE / self.size;
        let metadata_slots = 64.min(objects_per_page);

        for i in 0..metadata_slots {
            let obj_addr = base + (i * self.size);
            self.free_list[i] = Some(obj_addr);
            self.free_count += 1;
        }

        self.free_count -= 1;
        let addr = self.free_list[self.free_count].take().ok_or(Status::NoMemory)?;
        Ok(addr)
    }

    fn free(&mut self, addr: usize) -> Result<()> {
        if self.free_count < 64 {
            self.free_list[self.free_count] = Some(addr);
            self.free_count += 1;
            Ok(())
        } else {
            Err(Status::NoMemory)
        }
    }

    fn can_allocate(&self) -> bool {
        self.free_count > 0
    }
}

pub struct SlabAllocator {
    classes: [SlabClass; SLAB_CLASS_COUNT],
}

impl SlabAllocator {
    pub const fn new() -> Self {
        SlabAllocator {
            classes: [
                SlabClass::new(SLAB_SIZES[0]),
                SlabClass::new(SLAB_SIZES[1]),
                SlabClass::new(SLAB_SIZES[2]),
                SlabClass::new(SLAB_SIZES[3]),
                SlabClass::new(SLAB_SIZES[4]),
                SlabClass::new(SLAB_SIZES[5]),
                SlabClass::new(SLAB_SIZES[6]),
                SlabClass::new(SLAB_SIZES[7]),
            ],
        }
    }

    fn find_class_index(size: usize) -> Option<usize> {
        for i in 0..SLAB_CLASS_COUNT {
            if size <= SLAB_SIZES[i] {
                return Some(i);
            }
        }
        None
    }

    pub fn alloc(&mut self, size: usize) -> Result<usize> {
        let idx = Self::find_class_index(size).ok_or(Status::InvalidArgs)?;
        self.classes[idx].allocate()
    }

    pub fn free(&mut self, addr: usize, size: usize) -> Result<()> {
        let idx = Self::find_class_index(size).ok_or(Status::InvalidArgs)?;
        self.classes[idx].free(addr)
    }
}

static mut SLAB_ALLOCATOR: SlabAllocator = SlabAllocator::new();

pub fn slab_alloc(size: usize) -> Result<usize> {
    unsafe { SLAB_ALLOCATOR.alloc(size) }
}

pub fn slab_free(addr: usize, size: usize) -> Result<()> {
    unsafe { SLAB_ALLOCATOR.free(addr, size) }
}
