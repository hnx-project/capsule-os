use core::fmt::{self, Write};

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

#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!("\x1b[1;32m[  INFO ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::kprintln!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!("\x1b[1;33m[  WARN ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::kprintln!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!("\x1b[1;31m[ ERROR ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::kprintln!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        $crate::kprint!("\x1b[90m[ DEBUG ]\x1b[0m [\x1b[1;36m{:<6}\x1b[0m] ", $target);
        $crate::kprintln!($($arg)*);
    };
}
