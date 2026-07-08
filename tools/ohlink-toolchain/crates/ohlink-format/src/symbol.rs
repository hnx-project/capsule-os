use crate::FormatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolType {
    None = 0,
    Function = 1,
    Data = 2,
    File = 3,
}

impl SymbolType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => SymbolType::Function,
            2 => SymbolType::Data,
            3 => SymbolType::File,
            _ => SymbolType::None,
        }
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OHLK_Symbol {
    pub name_offset: u64,  // Offset in String Table segment
    pub ty: u8,            // 0=None, 1=Function, 2=Data, 3=File
    pub binding: u8,       // 0=Local, 1=Global, 2=Weak
    pub section_idx: u16,  // Header Entry Index (0xFFFF = Undefined/External)
    pub value: u64,        // Address or section offset
    pub size: u32,         // Symbol size
}

impl OHLK_Symbol {
    pub const SIZE: usize = 24;
    pub const UNDEFINED_SECTION: u16 = 0xFFFF;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.name_offset.to_le_bytes());
        bytes[8] = self.ty;
        bytes[9] = self.binding;
        bytes[10..12].copy_from_slice(&self.section_idx.to_le_bytes());
        bytes[12..20].copy_from_slice(&self.value.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.size.to_le_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < Self::SIZE {
            return Err(FormatError::BufferTooSmall);
        }
        let name_offset = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let ty = bytes[8];
        let binding = bytes[9];
        let section_idx = u16::from_le_bytes([bytes[10], bytes[11]]);
        let value = u64::from_le_bytes([
            bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19],
        ]);
        let size = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);

        Ok(Self {
            name_offset,
            ty,
            binding,
            section_idx,
            value,
            size,
        })
    }
}
