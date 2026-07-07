use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SegmentType {
    Text = 0x00010001,
    Data = 0x00020002,
    Rodata = 0x00030003,
    Bss = 0x00040004,
    Symtab = 0x00100010,
    Strtab = 0x00110011,
    Reloc = 0x00120012,
    Dynamic = 0x00200020,
    Custom(u32) = 0x80000000, // Range starts with 0x80000000
}

impl SegmentType {
    pub fn from_u32(val: u32) -> Self {
        match val {
            0x00010001 => SegmentType::Text,
            0x00020002 => SegmentType::Data,
            0x00030003 => SegmentType::Rodata,
            0x00040004 => SegmentType::Bss,
            0x00100010 => SegmentType::Symtab,
            0x00110011 => SegmentType::Strtab,
            0x00120012 => SegmentType::Reloc,
            0x00200020 => SegmentType::Dynamic,
            other => SegmentType::Custom(other),
        }
    }

    pub fn to_u32(&self) -> u32 {
        match *self {
            SegmentType::Text => 0x00010001,
            SegmentType::Data => 0x00020002,
            SegmentType::Rodata => 0x00030003,
            SegmentType::Bss => 0x00040004,
            SegmentType::Symtab => 0x00100010,
            SegmentType::Strtab => 0x00110011,
            SegmentType::Reloc => 0x00120012,
            SegmentType::Dynamic => 0x00200020,
            SegmentType::Custom(val) => val,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Entry {
    pub ty: u32,         // Segment type (SegmentType as u32)
    pub flags: u32,      // bit0=R, bit1=W, bit2=X
    pub offset: u64,     // Offset in file
    pub file_size: u64,  // Size of initialized data in file
    pub mem_size: u64,   // Size of segment in memory
}

impl OHLK_Entry {
    pub const SIZE: usize = 32;

    pub const FLAG_R: u32 = 1 << 0;
    pub const FLAG_W: u32 = 1 << 1;
    pub const FLAG_X: u32 = 1 << 2;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&self.ty.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.flags.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.offset.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.file_size.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.mem_size.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < Self::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let ty = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let flags = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let offset = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let file_size = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let mem_size = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
        ]);

        Ok(Self {
            ty,
            flags,
            offset,
            file_size,
            mem_size,
        })
    }
}
