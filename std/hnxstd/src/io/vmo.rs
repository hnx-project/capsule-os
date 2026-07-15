use shared::status::Status;

/// Safer Object Wrapper for Virtual Memory Objects (VMO)
pub struct Vmo {
    handle: usize,
}

impl Vmo {
    /// Wrapping an existing raw VMO handle
    pub const unsafe fn from_raw_handle(handle: usize) -> Self {
        Self { handle }
    }

    /// Retrieve the underlying raw handle value
    pub fn handle(&self) -> usize {
        self.handle
    }

    /// Create a child VMO slicing an existing parent VMO range
    pub fn create_child(&self, offset: usize, size: usize) -> Result<Self, Status> {
        let child_handle = hnxlibc::syscalls::vmo_create_child(self.handle, offset, size)?;
        // TEMP print the raw child_handle
        let bytes = [
            b'C', b'H', b'=',
            b'0' + (child_handle / 1000 % 10) as u8,
            b'0' + (child_handle / 100 % 10) as u8,
            b'0' + (child_handle / 10 % 10) as u8,
            b'0' + (child_handle % 10) as u8,
            b'\n'
        ];
        let _ = hnxlibc::write(1, bytes.as_ptr(), 8);
        Ok(Self {
            handle: child_handle as usize,
        })
    }

    /// Safely read a struct of type `T` from the VMO at specific byte `offset`
    pub fn read_struct<T: Copy>(&self, offset: usize) -> Result<T, Status> {
        let mut temp = core::mem::MaybeUninit::<T>::uninit();
        let size = core::mem::size_of::<T>();

        let buf_ptr = temp.as_mut_ptr() as *mut u8;
        let slice = unsafe { core::slice::from_raw_parts_mut(buf_ptr, size) };

        hnxlibc::syscalls::vmo_read(self.handle, offset, slice)?;

        unsafe { Ok(temp.assume_init()) }
    }

    /// Read raw slice buffer bytes
    pub fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Status> {
        hnxlibc::syscalls::vmo_read(self.handle, offset, buf)
    }

    /// Write raw slice buffer bytes
    pub fn write(&self, offset: usize, buf: &[u8]) -> Result<usize, Status> {
        hnxlibc::syscalls::vmo_write(self.handle, offset, buf)
    }
}

impl Drop for Vmo {
    fn drop(&mut self) {
        let _ = hnxlibc::syscalls::close(self.handle);
    }
}
