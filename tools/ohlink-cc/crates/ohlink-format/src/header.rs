use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Header {
    pub magic: u32,          // "OHLK" = 0x4F484C4B
    pub version_major: u16,  // 1
    pub version_minor: u16,  // 0
    pub endian: u8,         // 0 = Little Endian
    pub arch: u8,           // 1=ARM64, 2=x86_64, 3=RISC-V64
    pub header_count: u16,  // Header Entry count
    pub header_offset: u32,  // Start offset of header table (usually 0x30)
    pub data_offset: u32,    // Start offset of data section
    pub file_size: u64,      // Total file size
    pub checksum: u32,       // CRC32 of the file (excluding this field)
    pub flags: u32,          // Flags bitmask
    pub reserved: [u8; 12],  // Future extension
}

impl OHLK_Header {
    pub const MAGIC: u32 = 0x4F484C4B; // "OHLK"
    pub const SIZE: usize = 48;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&self.magic.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.version_major.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.version_minor.to_le_bytes());
        bytes[8] = self.endian;
        bytes[9] = self.arch;
        bytes[10..12].copy_from_slice(&self.header_count.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.header_offset.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.data_offset.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.file_size.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.flags.to_le_bytes());
        bytes[36..48].copy_from_slice(&self.reserved);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < Self::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != Self::MAGIC {
            return Err(FormatError::InvalidMagic);
        }
        let version_major = u16::from_le_bytes([bytes[4], bytes[5]]);
        let version_minor = u16::from_le_bytes([bytes[6], bytes[7]]);
        if version_major != 1 {
            return Err(FormatError::InvalidVersion);
        }
        let endian = bytes[8];
        let arch = bytes[9];
        let header_count = u16::from_le_bytes([bytes[10], bytes[11]]);
        let header_offset = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let data_offset = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let file_size = u64::from_le_bytes([
            bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
        ]);
        let checksum = u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        let flags = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
        let mut reserved = [0u8; 12];
        reserved.copy_from_slice(&bytes[36..48]);

        Ok(Self {
            magic,
            version_major,
            version_minor,
            endian,
            arch,
            header_count,
            header_offset,
            data_offset,
            file_size,
            checksum,
            flags,
            reserved,
        })
    }
}
