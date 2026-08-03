use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Header {
    pub magic: u32,
    pub version_major: u16,
    pub version_minor: u16,
    pub endian: u8,
    pub arch: u8,
    pub file_type: u16,
    pub header_count: u16,
    pub header_offset: u32,
    pub data_offset: u32,
    pub file_size: u64,
    pub checksum: u32,
    pub flags: u32,
    pub entry_point: u64,
    pub reserved: [u8; 18],
}

impl OHLK_Header {
    pub const MAGIC: u32 = 0x4F484C4B;
    pub const SIZE: usize = 64;
    pub const HEADER_OFFSET: u32 = 0x40;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&self.magic.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.version_major.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.version_minor.to_le_bytes());
        bytes[8] = self.endian;
        bytes[9] = self.arch;
        bytes[10..12].copy_from_slice(&self.file_type.to_le_bytes());
        bytes[12..14].copy_from_slice(&self.header_count.to_le_bytes());
        bytes[14..18].copy_from_slice(&self.header_offset.to_le_bytes());
        bytes[18..22].copy_from_slice(&self.data_offset.to_le_bytes());
        bytes[22..30].copy_from_slice(&self.file_size.to_le_bytes());
        bytes[30..34].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[34..38].copy_from_slice(&self.flags.to_le_bytes());
        bytes[38..46].copy_from_slice(&self.entry_point.to_le_bytes());
        bytes[46..64].copy_from_slice(&self.reserved);
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
        let file_type = u16::from_le_bytes([bytes[10], bytes[11]]);
        let header_count = u16::from_le_bytes([bytes[12], bytes[13]]);
        let header_offset = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
        let data_offset = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let file_size = u64::from_le_bytes([
            bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29],
        ]);
        let checksum = u32::from_le_bytes([bytes[30], bytes[31], bytes[32], bytes[33]]);
        let flags = u32::from_le_bytes([bytes[34], bytes[35], bytes[36], bytes[37]]);
        let entry_point = u64::from_le_bytes([
            bytes[38], bytes[39], bytes[40], bytes[41], bytes[42], bytes[43], bytes[44], bytes[45],
        ]);
        let mut reserved = [0u8; 18];
        reserved.copy_from_slice(&bytes[46..64]);

        Ok(Self {
            magic,
            version_major,
            version_minor,
            endian,
            arch,
            file_type,
            header_count,
            header_offset,
            data_offset,
            file_size,
            checksum,
            flags,
            entry_point,
            reserved,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FileType {
    Executable = 1,
    Relocatable = 2,
    Shared = 3,
    KernelModule = 4,
    Firmware = 5,
    Bootloader = 6,
}

impl FileType {
    pub fn from_u16(val: u16) -> Self {
        match val {
            1 => FileType::Executable,
            2 => FileType::Relocatable,
            3 => FileType::Shared,
            4 => FileType::KernelModule,
            5 => FileType::Firmware,
            6 => FileType::Bootloader,
            _ => FileType::Relocatable,
        }
    }

    pub fn to_u16(&self) -> u16 {
        *self as u16
    }
}
