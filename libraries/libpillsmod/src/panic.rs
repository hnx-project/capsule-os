//! # 🚨 Unified PillsMod Panic Runtime Handler
//!
//! Provides the fallback or dynamic panic handler for freestanding
//! `no_std` EL1 kernel extension modules.

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Static holder of the imported kernel logging function pointer to print panic traces.
pub static KERNEL_LOG_FN: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "panic-handler")]
#[inline(never)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let log_fn_addr = KERNEL_LOG_FN.load(Ordering::Relaxed);
    if log_fn_addr != 0 {
        let log_write: extern "C" fn(*const u8, usize, *const u8, usize) = unsafe {
            core::mem::transmute(log_fn_addr)
        };
        let tag = b"PILL_PANIC\0";
        let msg = b"PillsMod kernel driver panicked in EL1!\0";
        log_write(tag.as_ptr(), 10, msg.as_ptr(), 40);
    }
    loop {}
}
