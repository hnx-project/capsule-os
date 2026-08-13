//! # 📝 Dynamic Logging and Formatting Engine for PillsMod
//!
//! Exposes easy-to-use logging macros that dynamic EL1 modules can use
//! to write formatted strings back into the central kernel console buffer.

use core::fmt::{Write, Result};
use core::sync::atomic::Ordering;
use crate::panic::KERNEL_LOG_FN;

/// Formatting helper writing directly to the kernel's dynamic logger callback.
pub struct PillConsoleWriter;

impl Write for PillConsoleWriter {
    fn write_str(&mut self, s: &str) -> Result {
        let log_fn_addr = KERNEL_LOG_FN.load(Ordering::Relaxed);
        if log_fn_addr != 0 {
            let log_write: extern "C" fn(*const u8, usize, *const u8, usize) = unsafe {
                core::mem::transmute(log_fn_addr)
            };
            let tag = b"PILL\0";
            log_write(tag.as_ptr(), 4, s.as_ptr(), s.len());
        }
        Ok(())
    }
}

/// Dynamic console print helper macro for PillsMod.
#[macro_export]
macro_rules! pill_log {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = $crate::log::PillConsoleWriter;
        let _ = core::write!(&mut writer, $($arg)*);
        let _ = writer.write_str("\n");
    }};
}

/// Direct print function with zero formatting or stack overhead.
/// Invokes the kernel log callback directly with a static/dynamic tag and message.
pub fn pill_print(kernel: &crate::KernelImportTable, msg: &str) {
    let tag = b"PILL\0";
    (kernel.log_write)(tag.as_ptr(), 4, msg.as_ptr(), msg.len());
}
