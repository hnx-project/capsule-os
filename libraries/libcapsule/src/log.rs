use core::fmt::{self, Write};

/// A stateless debug writer that outputs characters directly to the kernel
/// UART debugger port via SYSCALL_WRITE (fd 1).
/// Completely independent of any thread-local or global POSIX FdTable.
pub struct DebugWriter;

impl Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.is_empty() {
            return Ok(());
        }
        // Direct kernel UART debug sys_write bypass.
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

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!(
            "\x1b[1;32m{}\x1b[36m{:<8}\x1b[0m | ",
            $crate::log::LEVEL_PAD_INFO,
            $crate::log::truncate_to_8(concat!("U-", $target))
        );
        $crate::kprintln!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!(
            "\x1b[1;33m{}\x1b[36m{:<8}\x1b[0m | ",
            $crate::log::LEVEL_PAD_WARN,
            $crate::log::truncate_to_8(concat!("U-", $target))
        );
        $crate::kprintln!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!(
            "\x1b[1;31m{}\x1b[36m{:<8}\x1b[0m | ",
            $crate::log::LEVEL_PAD_ERROR,
            $crate::log::truncate_to_8(concat!("U-", $target))
        );
        $crate::kprintln!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            $crate::kprint!(
                "\x1b[90m{}\x1b[36m{:<8}\x1b[0m | ",
                $crate::log::LEVEL_PAD_DEBUG,
                $crate::log::truncate_to_8(concat!("U-", $target))
            );
            $crate::kprintln!($($arg)*);
        }
    };
}
