use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RelocType {
    Abs64 = 1,
    Call26 = 2,
    AdrPrelPgHi21 = 3,
    AddAbsLo12Nc = 4,
    Ldst64AbsLo12Nc = 5,
    Ldst32AbsLo12Nc = 6,
    Custom(u32),
}

impl RelocType {
    pub fn from_u32(val: u32) -> Self {
        match val {
            1 => RelocType::Abs64,
            2 => RelocType::Call26,
            3 => RelocType::AdrPrelPgHi21,
            4 => RelocType::AddAbsLo12Nc,
            5 => RelocType::Ldst64AbsLo12Nc,
            6 => RelocType::Ldst32AbsLo12Nc,
            other => RelocType::Custom(other),
        }
    }

    pub fn to_u32(&self) -> u32 {
        match *self {
            RelocType::Abs64 => 1,
            RelocType::Call26 => 2,
            RelocType::AdrPrelPgHi21 => 3,
            RelocType::AddAbsLo12Nc => 4,
            RelocType::Ldst64AbsLo12Nc => 5,
            RelocType::Ldst32AbsLo12Nc => 6,
            RelocType::Custom(val) => val,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Reloc {
    pub offset: u64,
    pub ty: u32,
    pub symbol_idx: u32,
    pub bit_size: u8,
    pub reserved8: u8,
    pub reserved16: u16,
    pub addend: i64,
    pub reserved: [u8; 4],
}

impl OHLK_Reloc {
    pub const SIZE: usize = 32;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.offset.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.ty.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.symbol_idx.to_le_bytes());
        bytes[16] = self.bit_size;
        bytes[17] = self.reserved8;
        bytes[18..20].copy_from_slice(&self.reserved16.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.addend.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < Self::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let offset = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let ty = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let symbol_idx = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let bit_size = bytes[16];
        let reserved8 = bytes[17];
        let reserved16 = u16::from_le_bytes([bytes[18], bytes[19]]);
        let addend = i64::from_le_bytes([
            bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
        ]);
        let mut reserved = [0u8; 4];
        reserved.copy_from_slice(&bytes[28..32]);

        Ok(Self {
            offset,
            ty,
            symbol_idx,
            bit_size,
            reserved8,
            reserved16,
            addend,
            reserved,
        })
    }
}
