use core::time::Duration;

pub fn sleep(duration: Duration) {
    let req = [duration.as_secs() as u32, duration.subsec_nanos()];
    let _ = libc::nanosleep(req.as_ptr() as *const u8, core::ptr::null_mut());
}

pub fn yield_now() {
    libcapsule::syscalls::yield_cpu();
}

pub fn spawn<F, T>(_f: F)
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    panic!("CapsuleOS: std::thread::spawn() is not supported yet on microkernel");
}
