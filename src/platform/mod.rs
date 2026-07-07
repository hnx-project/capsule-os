#[cfg(feature = "virt")]
mod virt;
#[cfg(feature = "virt")]
pub use virt::*;

use core::fmt::{self, Write};

/// 实现全局格式化打印支持
pub fn print_fmt(args: fmt::Arguments) {
    let mut writer = PlatformWriter;
    let _ = writer.write_fmt(args);
}

struct PlatformWriter;

impl Write for PlatformWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            putchar(b);
        }
        Ok(())
    }
}

// 导出全局宏
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::platform::print_fmt(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => {
        $crate::platform::print_fmt(format_args!("{}\n", format_args!($($arg)*)));
    };
}

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::print!("\x1b[1;32m[  INFO ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::println!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::print!("\x1b[1;33m[  WARN ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::println!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::print!("\x1b[1;31m[ ERROR ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::println!($($arg)*);
    };
}
