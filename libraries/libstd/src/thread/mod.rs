use core::time::Duration;

pub fn sleep(duration: Duration) {
    let req = libc::timespec {
        tv_sec: duration.as_secs() as i64,
        tv_nsec: duration.subsec_nanos() as i64,
    };
    let _ = libc::nanosleep(&req as *const libc::timespec, core::ptr::null_mut());
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
