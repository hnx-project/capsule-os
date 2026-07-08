use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SegmentType {
    Text = 1,
    Data = 2,
    Rodata = 3,
    Bss = 4,
    Symtab = 5,
    Strtab = 6,
    Reloc = 7,
    Dynamic = 8,
    Import = 9,
    Export = 10,
    Got = 11,
    Plt = 12,
    Hash = 13,
    Tls = 14,
    Init = 15,
    Fini = 16,
    Debug = 17,
    Line = 18,
    Custom(u32) = 0x80000000,
}

impl SegmentType {
    pub fn from_u32(val: u32) -> Self {
        match val {
            1 => SegmentType::Text,
            2 => SegmentType::Data,
            3 => SegmentType::Rodata,
            4 => SegmentType::Bss,
            5 => SegmentType::Symtab,
            6 => SegmentType::Strtab,
            7 => SegmentType::Reloc,
            8 => SegmentType::Dynamic,
            9 => SegmentType::Import,
            10 => SegmentType::Export,
            11 => SegmentType::Got,
            12 => SegmentType::Plt,
            13 => SegmentType::Hash,
            14 => SegmentType::Tls,
            15 => SegmentType::Init,
            16 => SegmentType::Fini,
            17 => SegmentType::Debug,
            18 => SegmentType::Line,
            other => SegmentType::Custom(other),
        }
    }

    pub fn to_u32(&self) -> u32 {
        match *self {
            SegmentType::Text => 1,
            SegmentType::Data => 2,
            SegmentType::Rodata => 3,
            SegmentType::Bss => 4,
            SegmentType::Symtab => 5,
            SegmentType::Strtab => 6,
            SegmentType::Reloc => 7,
            SegmentType::Dynamic => 8,
            SegmentType::Import => 9,
            SegmentType::Export => 10,
            SegmentType::Got => 11,
            SegmentType::Plt => 12,
            SegmentType::Hash => 13,
            SegmentType::Tls => 14,
            SegmentType::Init => 15,
            SegmentType::Fini => 16,
            SegmentType::Debug => 17,
            SegmentType::Line => 18,
            SegmentType::Custom(val) => val,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Entry {
    pub ty: u32,
    pub flags: u32,
    pub file_offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub mem_size: u64,
    pub alignment: u64,
    pub link: u32,
    pub info: u32,
    pub name_offset: u32,
}

impl OHLK_Entry {
    pub const SIZE: usize = 56;

    pub const FLAG_R: u32 = 1 << 0;
    pub const FLAG_W: u32 = 1 << 1;
    pub const FLAG_X: u32 = 1 << 2;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&self.ty.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.flags.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.file_offset.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.virtual_address.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.file_size.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.mem_size.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.alignment.to_le_bytes());
        bytes[48..52].copy_from_slice(&self.link.to_le_bytes());
        bytes[52..56].copy_from_slice(&self.info.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < Self::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let ty = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let flags = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let file_offset = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let virtual_address = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let file_size = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
        ]);
        let mem_size = u64::from_le_bytes([
            bytes[32], bytes[33], bytes[34], bytes[35], bytes[36], bytes[37], bytes[38], bytes[39],
        ]);
        let alignment = u64::from_le_bytes([
            bytes[40], bytes[41], bytes[42], bytes[43], bytes[44], bytes[45], bytes[46], bytes[47],
        ]);
        let link = u32::from_le_bytes([bytes[48], bytes[49], bytes[50], bytes[51]]);
        let info = u32::from_le_bytes([bytes[52], bytes[53], bytes[54], bytes[55]]);
        let name_offset = u32::from_le_bytes([bytes[56], bytes[57], bytes[58], bytes[59]]);

        Ok(Self {
            ty,
            flags,
            file_offset,
            virtual_address,
            file_size,
            mem_size,
            alignment,
            link,
            info,
            name_offset,
        })
    }
}
