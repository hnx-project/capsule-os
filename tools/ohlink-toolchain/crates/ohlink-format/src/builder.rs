extern crate alloc;
use alloc::vec::Vec;
use crate::{FormatError, OHLK_Header, OHLK_Entry, crc32_ieee};

pub struct OHLK_Builder {
    header: OHLK_Header,
    entries: Vec<OHLK_Entry>,
    sections_data: Vec<Vec<u8>>,
}

impl OHLK_Builder {
    pub fn new(arch: u8, file_type: u16, flags: u32, entry_point: u64) -> Self {
        let header = OHLK_Header {
            magic: OHLK_Header::MAGIC,
            version_major: 1,
            version_minor: 1,
            endian: 0,
            arch,
            file_type,
            header_count: 0,
            header_offset: OHLK_Header::HEADER_OFFSET,
            data_offset: 0,
            file_size: 0,
            checksum: 0,
            flags,
            entry_point,
            reserved: [0; 18],
        };

        Self {
            header,
            entries: Vec::new(),
            sections_data: Vec::new(),
        }
    }

    pub fn add_segment(
        &mut self,
        ty: u32,
        flags: u32,
        data: &[u8],
        mem_size: u64,
        virtual_address: u64,
        alignment: u64,
    ) -> u16 {
        let index = self.entries.len() as u16;
        let entry = OHLK_Entry {
            ty,
            flags,
            file_offset: 0,
            virtual_address,
            file_size: data.len() as u64,
            mem_size,
            alignment,
            link: 0,
            info: 0,
            name_offset: 0,
        };
        self.entries.push(entry);
        self.sections_data.push(Vec::from(data));
        self.header.header_count = self.entries.len() as u16;
        index
    }

    pub fn build(mut self) -> Result<Vec<u8>, FormatError> {
        let num_entries = self.entries.len();
        let header_table_size = num_entries * OHLK_Entry::SIZE;
        let data_start_offset = OHLK_Header::SIZE + header_table_size;

        self.header.data_offset = data_start_offset as u32;

        let mut current_offset = data_start_offset as u64;
        let mut final_buf = Vec::new();

        final_buf.resize(data_start_offset, 0);

        for i in 0..num_entries {
            let data_len = self.sections_data[i].len() as u64;
            let alignment = if self.entries[i].alignment > 0 {
                self.entries[i].alignment
            } else {
                16
            };
            let aligned_offset = (current_offset + alignment - 1) & !(alignment - 1);

            if aligned_offset > current_offset {
                let padding = (aligned_offset - current_offset) as usize;
                final_buf.resize(final_buf.len() + padding, 0);
            }

            self.entries[i].file_offset = aligned_offset;
            final_buf.extend_from_slice(&self.sections_data[i]);
            current_offset = aligned_offset + data_len;
        }

        self.header.file_size = final_buf.len() as u64;

        let header_bytes = self.header.to_bytes();
        final_buf[0..OHLK_Header::SIZE].copy_from_slice(&header_bytes);

        for (i, entry) in self.entries.iter().enumerate() {
            let entry_bytes = entry.to_bytes();
            let entry_offset = OHLK_Header::SIZE + (i * OHLK_Entry::SIZE);
            final_buf[entry_offset..(entry_offset + OHLK_Entry::SIZE)].copy_from_slice(&entry_bytes);
        }

        final_buf[30..34].copy_from_slice(&[0, 0, 0, 0]);
        let checksum_val = crc32_ieee(&final_buf);
        final_buf[30..34].copy_from_slice(&checksum_val.to_le_bytes());

        Ok(final_buf)
    }
}
