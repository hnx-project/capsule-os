use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolType {
    None = 0,
    Function = 1,
    Data = 2,
    File = 3,
    Section = 4,
}

impl SymbolType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => SymbolType::Function,
            2 => SymbolType::Data,
            3 => SymbolType::File,
            4 => SymbolType::Section,
            _ => SymbolType::None,
        }
    }

    pub fn to_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolBinding {
    Local = 0,
    Global = 1,
    Weak = 2,
}

impl SymbolBinding {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => SymbolBinding::Global,
            2 => SymbolBinding::Weak,
            _ => SymbolBinding::Local,
        }
    }

    pub fn to_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolVisibility {
    Default = 0,
    Hidden = 1,
    Protected = 2,
}

impl SymbolVisibility {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => SymbolVisibility::Hidden,
            2 => SymbolVisibility::Protected,
            _ => SymbolVisibility::Default,
        }
    }

    pub fn to_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Symbol {
    pub name_offset: u64,
    pub value: u64,
    pub size: u64,
    pub ty: u8,
    pub binding: u8,
    pub visibility: u8,
    pub section_idx: u16,
    pub reserved: [u8; 21],
}

impl OHLK_Symbol {
    pub const SIZE: usize = 32;
    pub const UNDEFINED_SECTION: u16 = 0xFFFF;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.name_offset.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.value.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.size.to_le_bytes());
        bytes[24] = self.ty;
        bytes[25] = self.binding;
        bytes[26] = self.visibility;
        bytes[27..29].copy_from_slice(&self.section_idx.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < Self::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let name_offset = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let value = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let size = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        ]);
        let ty = bytes[24];
        let binding = bytes[25];
        let visibility = bytes[26];
        let section_idx = u16::from_le_bytes([bytes[27], bytes[28]]);
        let mut reserved = [0u8; 21];
        reserved.copy_from_slice(&bytes[29..32]);

        Ok(Self {
            name_offset,
            value,
            size,
            ty,
            binding,
            visibility,
            section_idx,
            reserved,
        })
    }
}
