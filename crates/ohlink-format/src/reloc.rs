use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RelocType {
    Abs64 = 1,                 // R_AARCH64_ABS64: S + A
    Call26 = 2,                // R_AARCH64_CALL26: (S + A - P) >> 2
    AdrPrelPgHi21 = 3,         // R_AARCH64_ADR_PREL_PG_HI21: (Page(S+A) - Page(P)) >> 12
    AddAbsLo12Nc = 4,          // R_AARCH64_ADD_ABS_LO12_NC: (S + A) & 0xFFF
    Ldst64AbsLo12Nc = 5,       // R_AARCH64_LDST64_ABS_LO12_NC: ((S + A) & 0xFFF) >> 3
    Ldst32AbsLo12Nc = 6,       // R_AARCH64_LDST32_ABS_LO12_NC: ((S + A) & 0xFFF) >> 2
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
    pub offset: u64,       // Offset in target section
    pub ty: u32,           // RelocType as u32
    pub symbol_idx: u32,   // Symbol index in Symbol Table
    pub addend: i64,       // Explicit addend
    pub section_idx: u16,  // Target section index in Header Entry Table (e.g. .text index)
    pub reserved: [u8; 6], // Padding to 32 bytes
}

impl OHLK_Reloc {
    pub const SIZE: usize = 32;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.offset.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.ty.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.symbol_idx.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.addend.to_le_bytes());
        bytes[24..26].copy_from_slice(&self.section_idx.to_le_bytes());
        bytes[26..32].copy_from_slice(&self.reserved);
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
        let addend = i64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let section_idx = u16::from_le_bytes([bytes[24], bytes[25]]);
        let mut reserved = [0u8; 6];
        reserved.copy_from_slice(&bytes[26..32]);

        Ok(Self {
            offset,
            ty,
            symbol_idx,
            addend,
            section_idx,
            reserved,
        })
    }
}
