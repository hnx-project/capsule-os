//! B9 (`KERNEL_HEALTH.md` B9): Vec<T> for the 1.0 demo.
//!
//! In 1.0 we deliberately back Vec with a fixed-size linear
//! buffer (256 bytes = 64 u32 / 256 u8) so we can ship Vec
//! without a global allocator.  The plan is for 1.1 to land a
//! `kernel/src/mm/slab.rs` global allocator and lift this
//! constraint.

pub struct Vec<T: Copy> {
    buf: [core::mem::MaybeUninit<T>; 64],
    len: usize,
}

impl<T: Copy> Vec<T> {
    pub const fn new() -> Self {
        Self {
            buf: [core::mem::MaybeUninit::uninit(); 64],
            len: 0,
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), &'static str> {
        if self.len >= self.buf.len() {
            return Err("Vec: capacity exceeded");
        }
        self.buf[self.len] = core::mem::MaybeUninit::new(value);
        self.len += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        let value = unsafe { core::ptr::read(self.buf[self.len].as_ptr()) };
        Some(value)
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe {
            core::slice::from_raw_parts(
                self.buf.as_ptr() as *const T,
                self.len,
            )
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.buf.as_mut_ptr() as *mut T,
                self.len,
            )
        }
    }
}
