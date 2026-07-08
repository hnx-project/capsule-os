extern crate alloc;
use alloc::vec::Vec;
use crate::{FormatError, OHLK_Header, OHLK_Entry, crc32_ieee};

/// Builder API for generating structurally sound and CRC32-checked OHLINK binaries.
pub struct OHLK_Builder {
    header: OHLK_Header,
    entries: Vec<OHLK_Entry>,
    sections_data: Vec<Vec<u8>>,
}

impl OHLK_Builder {
    pub fn new(arch: u8, flags: u32) -> Self {
        let header = OHLK_Header {
            magic: OHLK_Header::MAGIC,
            version_major: 1,
            version_minor: 0,
            endian: 0, // Little Endian
            arch,
            header_count: 0,
            header_offset: 0x30,
            data_offset: 0,
            file_size: 0,
            checksum: 0,
            flags,
            reserved: [0; 12],
        };

        Self {
            header,
            entries: Vec::new(),
            sections_data: Vec::new(),
        }
    }

    /// Adds a segment (such as .text or .data) to the binary.
    /// Returns the assigned section index.
    pub fn add_segment(&mut self, ty: u32, flags: u32, data: &[u8], mem_size: u64) -> u16 {
        let index = self.entries.len() as u16;
        let entry = OHLK_Entry {
            ty,
            flags,
            offset: 0, // Will be computed at build time
            file_size: data.len() as u64,
            mem_size,
        };
        self.entries.push(entry);
        self.sections_data.push(Vec::from(data));
        self.header.header_count = self.entries.len() as u16;
        index
    }

    /// Packs and aligns the binary, calculates the final OHLK_Header and CRC32 checksum.
    pub fn build(mut self) -> Result<Vec<u8>, FormatError> {
        let num_entries = self.entries.len();
        let header_table_size = num_entries * OHLK_Entry::SIZE;
        let data_start_offset = 0x30 + header_table_size;

        self.header.data_offset = data_start_offset as u32;

        let mut current_offset = data_start_offset as u64;
        let mut final_buf = Vec::new();

        // 1. Reserves Space for OHLK_Header (48 bytes) and Header Entry Table
        final_buf.resize(data_start_offset, 0);

        // 2. Lay out data and compute offsets for entries
        for i in 0..num_entries {
            let data_len = self.sections_data[i].len() as u64;
            
            // Align offsets to 16 bytes for proper ARM64 execution alignment
            let aligned_offset = (current_offset + 15) & !15;
            
            // Fill padding bytes
            if aligned_offset > current_offset {
                let padding = (aligned_offset - current_offset) as usize;
                final_buf.resize(final_buf.len() + padding, 0);
            }

            self.entries[i].offset = aligned_offset;
            final_buf.extend_from_slice(&self.sections_data[i]);
            current_offset = aligned_offset + data_len;
        }

        self.header.file_size = final_buf.len() as u64;

        // 3. Serialize Header and Entries into final buffer
        let header_bytes = self.header.to_bytes();
        final_buf[0..48].copy_from_slice(&header_bytes);

        for (i, entry) in self.entries.iter().enumerate() {
            let entry_bytes = entry.to_bytes();
            let entry_offset = 0x30 + (i * OHLK_Entry::SIZE);
            final_buf[entry_offset..(entry_offset + OHLK_Entry::SIZE)].copy_from_slice(&entry_bytes);
        }

        // 4. Calculate Checksum over whole file with checksum field zeroed
        // Zero checksum field first in the serialized header
        final_buf[28..32].copy_from_slice(&[0, 0, 0, 0]);
        let checksum_val = crc32_ieee(&final_buf);

        // Update checksum field in the buffer
        final_buf[28..32].copy_from_slice(&checksum_val.to_le_bytes());

        Ok(final_buf)
    }
}
