#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod crc32;
pub mod header;
pub mod entry;
pub mod symbol;
pub mod reloc;
pub mod parser;
pub mod builder;
#[cfg(test)]
pub mod tests;

pub use header::OHLK_Header;
pub use entry::{OHLK_Entry, SegmentType};
pub use symbol::{OHLK_Symbol, SymbolType, SymbolBinding};
pub use reloc::{OHLK_Reloc, RelocType};
pub use crc32::crc32_ieee;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    BufferTooSmall,
    InvalidMagic,
    InvalidVersion,
    ChecksumMismatch { expected: u32, calculated: u32 },
    InvalidEntryIndex,
    Utf8Error,
    ParseError,
    BuildError,
}
