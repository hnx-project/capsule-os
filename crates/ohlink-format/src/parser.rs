use crate::{FormatError, OHLK_Header, OHLK_Entry, crc32_ieee};

extern crate alloc;
use alloc::vec::Vec;

/// Parses an OHLINK binary from a byte slice.
/// Fully `no_std` compatible.
pub struct OHLK_Parser<'a> {
    data: &'a [u8],
    header: OHLK_Header,
}

impl<'a> OHLK_Parser<'a> {
    /// Validates magic numbers, checksum (CRC32), and basic file structure.
    pub fn new(data: &'a [u8]) -> Result<Self, FormatError> {
        if data.len() < OHLK_Header::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let header = OHLK_Header::from_bytes(data)?;
        if data.len() < header.file_size as usize {
            return Err(FormatError::BufferTooSmall);
        }

        // Verify Checksum (CRC32-IEEE)
        // Set checksum field to 0 for validation
        let expected_checksum = header.checksum;

        let mut data_copy = Vec::from(data);
        // Zero out checksum field (offset 28..32)
        data_copy[28..32].copy_from_slice(&[0, 0, 0, 0]);
        let calculated_checksum = crc32_ieee(&data_copy);

        if expected_checksum != calculated_checksum {
            return Err(FormatError::ChecksumMismatch {
                expected: expected_checksum,
                calculated: calculated_checksum,
            });
        }

        Ok(Self { data, header })
    }

    pub fn header(&self) -> &OHLK_Header {
        &self.header
    }

    /// Gets an Entry from the Header Table by index.
    pub fn get_entry(&self, index: u16) -> Result<OHLK_Entry, FormatError> {
        if index >= self.header.header_count {
            return Err(FormatError::InvalidEntryIndex);
        }
        let start = self.header.header_offset as usize + (index as usize * OHLK_Entry::SIZE);
        let end = start + OHLK_Entry::SIZE;
        if end > self.data.len() {
            return Err(FormatError::BufferTooSmall);
        }
        OHLK_Entry::from_bytes(&self.data[start..end])
    }

    /// Retrieves raw segment data using an entry.
    pub fn get_segment_data(&self, entry: &OHLK_Entry) -> Result<&'a [u8], FormatError> {
        let start = entry.offset as usize;
        let end = start + entry.file_size as usize;
        if end > self.data.len() {
            return Err(FormatError::BufferTooSmall);
        }
        Ok(&self.data[start..end])
    }

    /// Iterates over all header table entries.
    pub fn entries(&self) -> EntryIterator<'_, 'a> {
        EntryIterator {
            parser: self,
            current: 0,
            count: self.header.header_count,
        }
    }
}

pub struct EntryIterator<'parser, 'a> {
    parser: &'parser OHLK_Parser<'a>,
    current: u16,
    count: u16,
}

impl<'parser, 'a> Iterator for EntryIterator<'parser, 'a> {
    type Item = OHLK_Entry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.count {
            let entry = self.parser.get_entry(self.current).ok()?;
            self.current += 1;
            Some(entry)
        } else {
            None
        }
    }
}
