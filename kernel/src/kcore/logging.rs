use core::fmt::{self, Write};
use core::sync::atomic::{AtomicU8, Ordering};

pub const LEVEL_ERROR: u8 = 0;
pub const LEVEL_WARN: u8  = 1;
pub const LEVEL_INFO: u8  = 2;
pub const LEVEL_DEBUG: u8 = 3;

/// Console verbosity gate. Records with `level > LOG_LEVEL` are still
/// written to the ring buffer (see `kcore::logbuf`) but never reach the
/// UART.
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_INFO);

pub fn set_log_level(level: u8) {
    LOG_LEVEL.store(level, Ordering::Relaxed);
}

#[inline(always)]
pub fn get_log_level() -> u8 {
    LOG_LEVEL.load(Ordering::Relaxed)
}

/// Boot lifecycle marker.
///
/// | Phase | Meaning                                                |
/// |-------|--------------------------------------------------------|
/// |  0    | Kernel init only (FDT/MM/MMU). Full verbosity.         |
/// |  1    | User-space services are booting. Full verbosity.        |
/// |  2    | `svc.tty` has taken over the console. Only WARN/ERROR  |
/// |       | leak to the UART; everything else lives in the ring.    |
///
/// EL0 services promote this via the `SYS_LOG_SET_BOOT_PHASE` syscall.
pub const BOOT_PHASE_KERNEL: u8 = 0;
pub const BOOT_PHASE_SERVICES: u8 = 1;
pub const BOOT_PHASE_USERS_READY: u8 = 2;

static BOOT_PHASE: AtomicU8 = AtomicU8::new(BOOT_PHASE_KERNEL);

pub fn set_boot_phase(phase: u8) {
    BOOT_PHASE.store(phase, Ordering::Release);
}

#[inline(always)]
pub fn boot_phase() -> u8 {
    BOOT_PHASE.load(Ordering::Acquire)
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

/// Print directly to console bypassing the level gate. Used during early
/// boot before ring buffer initialisation.
#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n");
    };
    ($($arg:tt)*) => {
        $crate::kprint!($($arg)*);
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

/// Common builder used by every `log_*!` macro. Captures the formatted
/// output into a stack `LogCapture` whose `Drop` decides whether the
/// ring buffer + console get it.
#[macro_export]
macro_rules! __log_emit {
    ($level:expr, $target:expr, $prefix:expr, $($arg:tt)*) => {{
        let __target_full = $crate::kcore::logging::truncate_to_8(concat!("K-", $target));
        let mut __cap = $crate::kcore::logbuf::LogCapture::new(
            $level, __target_full, $prefix);
        let _ = core::fmt::write(&mut __cap, format_args!(
            "{:<8} | {}",
            __target_full, format_args!($($arg)*)));
        drop(__cap);
    }};
}

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::__log_emit!(
            $crate::kcore::logging::LEVEL_INFO, $target,
            Some(concat!("\x1b[1;32m", "INFO \x1b[0m | ", "\x1b[36m")),
            $($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::__log_emit!(
            $crate::kcore::logging::LEVEL_WARN, $target,
            Some(concat!("\x1b[1;33m", "WARN \x1b[0m | ", "\x1b[36m")),
            $($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::__log_emit!(
            $crate::kcore::logging::LEVEL_ERROR, $target,
            Some(concat!("\x1b[1;31m", "ERROR\x1b[0m | ", "\x1b[36m")),
            $($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        #[cfg(debug_assertions)]
        $crate::__log_emit!(
            $crate::kcore::logging::LEVEL_DEBUG, $target,
            Some(concat!("\x1b[90m", "DEBUG\x1b[0m | ", "\x1b[36m")),
            $($arg)*);
        // In release builds DEBUG records are dropped entirely — they
        // would otherwise flood the ring buffer with per-CPU TTBR0
        // churn and slow down replay / `dmesg` enumeration.  Pass
        // `make LOG_LEVEL=debug` to bring them back when chasing a bug.
    };
}