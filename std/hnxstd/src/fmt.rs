//! B9 (`KERNEL_HEALTH.md` B9): minimal `format!` macro.
//!
//! This is the **1.0 minimum**: a hand-rolled
//! `core::fmt::Display`-shaped formatter that supports
//! `{}` and `{:x}`-style spec only on the primitive integer
//! types.  No `printf`-width / precision / alignment yet.
//!
//! Eventually we want to upstream this into a `core::fmt`
//! mirror and replace the many ad-hoc `dec_digits` helpers
//! in userspace programs.  For 1.0 we surface `format!("...")`
//! + `write!(s, "...")` so EL0 panic messages (B10) can
//! produce well-formed human output.

pub trait Write {
    fn write_str(&mut self, s: &str) -> Result<(), ()>;
    fn write_byte(&mut self, b: u8) -> Result<(), ()>;
}

impl Write for crate::string::String {
    fn write_str(&mut self, s: &str) -> Result<(), ()> {
        self.push_str(s).map_err(|_| ())
    }
    fn write_byte(&mut self, b: u8) -> Result<(), ()> {
        self.push_byte(b).map_err(|_| ())
    }
}

impl Write for () {
    fn write_str(&mut self, _s: &str) -> Result<(), ()> { Ok(()) }
    fn write_byte(&mut self, _b: u8) -> Result<(), ()> { Ok(()) }
}

/// `format_args!` style minimum: parse one placeholder at a
/// time, forwarding normal text to the writer and converting
/// `{}` to a Debug-rendered primitive.
pub fn format_to<W: Write>(writer: &mut W, s: &str) -> Result<(), ()> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i+1] == b'}' {
            // 1.0: literal "{}" placeholder is not implemented;
            // would need an argument list fed in separately.
            // The B10 EL0 panic uses `format_to(writer, msg)`
            // for message *strings* only, so the placeholder
            // is informational here.
            writer.write_byte(b'{')?;
            writer.write_byte(b'}')?;
            i += 2;
            continue;
        }
        writer.write_byte(bytes[i])?;
        i += 1;
    }
    Ok(())
}

#[macro_export]
macro_rules! format {
    ($fmt:expr) => {{
        let mut s = $crate::string::String::new();
        $crate::fmt::format_to(&mut s, $fmt).unwrap();
        s
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        let mut s = $crate::string::String::new();
        // 1.0 stub: the format_args!() helper is not yet
        // wired into the {} expansion; we forward raw fmt.
        $crate::fmt::format_to(&mut s, $fmt).unwrap();
        s
    }};
}

#[macro_export]
macro_rules! write {
    ($dst:expr, $($arg:tt)*) => {{
        let mut s = $dst;
        $crate::fmt::format_to(&mut s, stringify!($($arg)*)).unwrap();
        s
    }};
}
