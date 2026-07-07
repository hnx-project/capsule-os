use shared::status::Status;

#[derive(Debug)]
pub struct ElfHeader {
    pub magic: [u8; 4],
    pub entry: usize,
}

pub struct ElfLoader;

impl ElfLoader {
    pub const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
    pub fn is_valid(_data: &[u8]) -> bool { true }
    pub fn parse_header(_data: &[u8]) -> Status { Status::Ok }
}
