#![no_std]

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub mod builder;
pub mod crc32;
pub mod entry;
pub mod header;
pub mod parser;
pub mod reloc;
pub mod symbol;
#[cfg(test)]
pub mod tests;

pub use crc32::crc32_ieee;
pub use entry::{OHLK_Entry, SegmentType};
pub use header::{FileType, OHLK_Header};
pub use reloc::{OHLK_Reloc, RelocType};
pub use symbol::{OHLK_Symbol, SymbolBinding, SymbolType, SymbolVisibility};

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
