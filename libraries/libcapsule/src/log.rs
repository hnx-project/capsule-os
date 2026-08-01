use core::fmt::{self, Write};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Kernel-side log level gate.  Updated by the kernel itself when
/// `set_log_level` is called via the `SYSCALL_LOGBUF_SET_LEVEL`
/// syscall; mirrored here so EL0 code can short-circuit before
/// trapping into the kernel for a log line that would be filtered
/// out anyway.
static EL0_LOG_LEVEL: AtomicUsize = AtomicUsize::new(2 /* INFO */);

/// Called by the kernel's `sys_logbuf_set_level` handler.  In a future
/// patch this will live inside a dedicated `SYSCALL_LOGBUF_GET_LEVEL`
/// read-back syscall; for now the boot image always boots at INFO.
pub fn set_el0_log_level(level: usize) {
    EL0_LOG_LEVEL.store(level, Ordering::Relaxed);
}

/// A stateless debug writer that outputs characters directly to the kernel
/// UART debugger port via SYSCALL_WRITE (fd 1).
/// Completely independent of any thread-local or global POSIX FdTable.
pub struct DebugWriter;

impl Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.is_empty() {
            return Ok(());
        }
        unsafe {
            let _ = crate::syscall!(
                shared::syscall_nums::SYSCALL_WRITE,
                1, // fd = 1 (Debug UART Stdout)
                s.as_ptr() as usize,
                s.len(),
                0,
                0,
                0
            );
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        let mut writer = $crate::log::DebugWriter;
        let _ = core::fmt::write(&mut writer, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => {{
        $crate::kprint!($($arg)*);
        $crate::kprint!("\n");
    }};
}

pub const LEVEL_PAD_INFO:  &str = "INFO \x1b[0m | ";
pub const LEVEL_PAD_WARN:  &str = "WARN \x1b[0m | ";
pub const LEVEL_PAD_ERROR: &str = "ERROR\x1b[0m | ";
pub const LEVEL_PAD_DEBUG: &str = "DEBUG\x1b[0m | ";

/// Truncates a string to at most 8 characters. Since all tag targets are compiled
/// ASCII literals, we can safely slice at 8 bytes without boundary issues.
#[inline(always)]
pub fn truncate_to_8(s: &str) -> &str {
    if s.len() <= 8 {
        s
    } else {
        &s[..8]
    }
}

/// Common emitter used by every `log_*!` macro: formats the message
/// into a stack buffer, then forwards through `SYSCALL_LOG_EMIT` so
/// the kernel can apply its boot-phase / log-level gate and decide
/// whether to also push the line to the console UART.
#[macro_export]
macro_rules! __log_emit_user {
    ($level:expr, $target:expr, $($arg:tt)*) => {{
        // Stack buffer for the formatted message (matches kernel
        // `LogCapture::MSG_MAX = 200`).
        let mut buf = [0u8; 200];
        let mut len = 0usize;
        {
            struct W<'a>(&'a mut [u8], &'a mut usize);
            impl<'a> core::fmt::Write for W<'a> {
                fn write_str(&mut self, s: &str) -> core::fmt::Result {
                    let bytes = s.as_bytes();
                    let take = core::cmp::min(bytes.len(), self.0.len() - *self.1);
                    self.0[*self.1..*self.1 + take].copy_from_slice(&bytes[..take]);
                    *self.1 += take;
                    Ok(())
                }
            }
            let mut w = W(&mut buf, &mut len);
            let _ = core::fmt::write(&mut w, format_args!($($arg)*));
        }
        let target_str = $crate::log::truncate_to_8(concat!("U-", $target));
        unsafe {
            let _ = $crate::syscall!(
                shared::syscall_nums::SYSCALL_LOG_EMIT,
                $level,
                target_str.as_ptr() as usize,
                target_str.len(),
                buf.as_ptr() as usize,
                len,
                0
            );
        }
    }};
}

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::__log_emit_user!(2, $target, $($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::__log_emit_user!(1, $target, $($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::__log_emit_user!(0, $target, $($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        #[cfg(debug_assertions)]
        $crate::__log_emit_user!(3, $target, $($arg)*);
    };
}
