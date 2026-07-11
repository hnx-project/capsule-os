use core::fmt::{self, Write};
use core::sync::atomic::{AtomicU8, Ordering};

pub const LEVEL_ERROR: u8 = 0;
pub const LEVEL_WARN: u8  = 1;
pub const LEVEL_INFO: u8  = 2;
pub const LEVEL_DEBUG: u8 = 3;

static LOG_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_INFO);

pub fn set_log_level(level: u8) {
    LOG_LEVEL.store(level, Ordering::Relaxed);
}

#[inline(always)]
pub fn get_log_level() -> u8 {
    LOG_LEVEL.load(Ordering::Relaxed)
}

pub struct ConsoleWriter;

impl Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                crate::arch::console_putchar(b'\r');
            }
            crate::arch::console_putchar(b);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        let _ = core::fmt::write(&mut $crate::kcore::logging::ConsoleWriter, format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n");
    };
    ($($arg:tt)*) => {
        let _ = core::fmt::write(&mut $crate::kcore::logging::ConsoleWriter, format_args!($($arg)*));
        $crate::kprint!("\n");
    };
}

/// Padded level+tag+message columns.  Two-space padding after the
/// 4-char INFO/WARN word plus the post-color reset keeps the leading
/// `|` lined up at column 7 for all four levels despite INFO/WARN
/// being one character shorter than ERROR/DEBUG.  The 14-char tag
/// column is wide enough to fit the longest tag seen in the tree
/// today — `SYSCALL_WRITE` at 13 chars + 1 padding space.
pub const LEVEL_PAD_INFO:  &str = "INFO \x1b[0m | ";
pub const LEVEL_PAD_WARN:  &str = "WARN \x1b[0m | ";
pub const LEVEL_PAD_ERROR: &str = "ERROR\x1b[0m | ";
pub const LEVEL_PAD_DEBUG: &str = "DEBUG\x1b[0m | ";

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        if $crate::kcore::logging::get_log_level() >= 2 {
            $crate::kprint!(
                "\x1b[1;32m{}\x1b[36m{:<14}\x1b[0m | ",
                $crate::kcore::logging::LEVEL_PAD_INFO,
                $target
            );
            $crate::kprintln!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        if $crate::kcore::logging::get_log_level() >= 1 {
            $crate::kprint!(
                "\x1b[1;33m{}\x1b[36m{:<14}\x1b[0m | ",
                $crate::kcore::logging::LEVEL_PAD_WARN,
                $target
            );
            $crate::kprintln!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        if $crate::kcore::logging::get_log_level() >= 0 {
            $crate::kprint!(
                "\x1b[1;31m{}\x1b[36m{:<14}\x1b[0m | ",
                $crate::kcore::logging::LEVEL_PAD_ERROR,
                $target
            );
            $crate::kprintln!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        if $crate::kcore::logging::get_log_level() >= 3 {
            $crate::kprint!(
                "\x1b[90m{}\x1b[36m{:<14}\x1b[0m | ",
                $crate::kcore::logging::LEVEL_PAD_DEBUG,
                $target
            );
            $crate::kprintln!($($arg)*);
        }
    };
}
