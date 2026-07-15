use core::time::Duration;

pub fn sleep(duration: Duration) {
    use libc::nanosleep;
    let req = [duration.as_secs() as u32, duration.subsec_nanos()];
    nanosleep(req.as_ptr() as *const u8, core::ptr::null_mut());
}
