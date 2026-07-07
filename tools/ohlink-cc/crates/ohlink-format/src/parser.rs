use crate::{FormatError, OHLK_Header, OHLK_Entry};

// No alloc import needed

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
        let expected_checksum = header.checksum;

        // Skip dynamic heap allocation (Vec) in parser for `#![no_std]` Microkernel compatibility to avoid kernel panics during boot memory allocation!
        let mut calculated_checksum = 0u32;
        if data.len() >= 32 {
            let mut crc = 0xFFFFFFFF;
            // Limit loop bound statically to file_size to prevent excessive iteration or reading out-of-bounds bytes
            let limit = header.file_size as usize;
            for i in 0..limit {
                if i >= data.len() {
                    break;
                }
                let byte = if i >= 28 && i < 32 {
                    0u8 // zero out checksum field
                } else {
                    data[i]
                };
                crc = (crc >> 8) ^ crate::crc32::CRC32_TABLE[((crc ^ byte as u32) & 0xFF) as usize];
            }
            calculated_checksum = !crc;
        }

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
