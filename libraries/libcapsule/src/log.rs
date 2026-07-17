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
