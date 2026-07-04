use core::alloc::Layout;
use core::ptr;

pub struct Vec<T> {
    ptr: *mut T,
    len: usize,
    cap: usize,
}

impl<T> Vec<T> {
    pub fn new() -> Self {
        Vec { ptr: ptr::null_mut(), len: 0, cap: 0 }
    }

    pub fn push(&mut self, value: T) {
        if self.len == self.cap {
            let new_cap = if self.cap == 0 { 1 } else { self.cap * 2 };
            let old_layout = Layout::array::<T>(self.cap).unwrap();
            let new_layout = Layout::array::<T>(new_cap).unwrap();
            let new_ptr = if self.cap == 0 {
                unsafe { core::alloc::alloc(new_layout) }
            } else {
                unsafe { core::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size()) }
            } as *mut T;
            self.ptr = new_ptr;
            self.cap = new_cap;
        }
        unsafe { ptr::write(self.ptr.add(self.len), value); }
        self.len += 1;
    }

    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
}

impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        unsafe {
            for i in 0..self.len { ptr::drop_in_place(self.ptr.add(i)); }
            if self.cap > 0 {
                core::alloc::dealloc(self.ptr as *mut u8, Layout::array::<T>(self.cap).unwrap());
            }
        }
    }
}
