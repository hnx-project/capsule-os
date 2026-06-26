#![no_std]

use core::fmt;

pub struct BootInfo {
    pub uart_base: usize,
    pub ram_base: usize,
    pub ram_size: usize,
}

impl BootInfo {
    pub const fn empty() -> Self {
        BootInfo {
            uart_base: 0,
            ram_base: 0,
            ram_size: 0,
        }
    }
}

impl fmt::Display for BootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  UART base : {:#x}", self.uart_base)?;
        writeln!(f, "  RAM base  : {:#x}", self.ram_base)?;
        writeln!(f, "  RAM size  : {:#x} ({} MB)", self.ram_size, self.ram_size / 1024 / 1024)
    }
}

const FDT_MAGIC: u32 = 0xD00DFEED;
const FDT_BEGIN_NODE: u32 = 0x00000001;
const FDT_END_NODE: u32 = 0x00000002;
const FDT_PROP: u32 = 0x00000003;
const FDT_NOP: u32 = 0x00000004;
const FDT_END: u32 = 0x00000009;

const DEFAULT_RAM_BASE: usize = 0x40000000;
const DEFAULT_RAM_SIZE: usize = 512 * 1024 * 1024;
const DEFAULT_UART_BASE: usize = 0x09000000;

#[inline(always)]
unsafe fn read_u32(p: *const u8) -> u32 {
    core::ptr::read_volatile(p as *const u32)
}

#[inline(always)]
unsafe fn read_u64(p: *const u8) -> u64 {
    core::ptr::read_volatile(p as *const u64)
}

fn align4(len: usize) -> usize {
    (len + 3) & !3
}

struct FdtHeader {
    magic: u32,
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    off_mem_rsvmap: u32,
    version: u32,
    last_comp_version: u32,
    boot_cpuid_phys: u32,
    size_dt_strings: u32,
    size_dt_struct: u32,
}

impl FdtHeader {
    unsafe fn parse(p: *const u8) -> Result<Self, &'static str> {
        let magic = read_u32(p.add(0)).to_be();
        if magic != FDT_MAGIC {
            return Err("Invalid FDT magic");
        }
        Ok(FdtHeader {
            magic,
            totalsize: read_u32(p.add(4)).to_be(),
            off_dt_struct: read_u32(p.add(8)).to_be(),
            off_dt_strings: read_u32(p.add(12)).to_be(),
            off_mem_rsvmap: read_u32(p.add(16)).to_be(),
            version: read_u32(p.add(20)).to_be(),
            last_comp_version: read_u32(p.add(24)).to_be(),
            boot_cpuid_phys: read_u32(p.add(28)).to_be(),
            size_dt_strings: read_u32(p.add(32)).to_be(),
            size_dt_struct: read_u32(p.add(36)).to_be(),
        })
    }
}

struct Parser<'a> {
    struct_block: &'a [u8],
    strings_block: &'a [u8],
}

impl<'a> Parser<'a> {
    fn new(base: *const u8, h: &FdtHeader) -> Self {
        unsafe {
            let struct_start = base.add(h.off_dt_struct as usize);
            let strings_start = base.add(h.off_dt_strings as usize);
            Parser {
                struct_block: core::slice::from_raw_parts(
                    struct_start,
                    h.size_dt_struct as usize,
                ),
                strings_block: core::slice::from_raw_parts(
                    strings_start,
                    h.size_dt_strings as usize,
                ),
            }
        }
    }

    fn get_string(&self, offset: usize) -> Option<&'a str> {
        if offset >= self.strings_block.len() {
            return None;
        }
        let mut end = offset;
        while end < self.strings_block.len() && self.strings_block[end] != 0 {
            end += 1;
        }
        core::str::from_utf8(&self.strings_block[offset..end]).ok()
    }

    fn find_node_property(&mut self, node_name: &str, prop_name: &str) -> Option<&'a [u8]> {
        let mut pos = 0usize;
        let mut depth = 0i32;
        let mut target_depth: Option<i32> = None;

        while pos < self.struct_block.len() {
            if pos + 4 > self.struct_block.len() {
                return None;
            }
            let token = unsafe { read_u32(self.struct_block.as_ptr().add(pos)).to_be() };
            pos += 4;

            match token {
                FDT_BEGIN_NODE => {
                    let name_start = pos;
                    let mut name_end = name_start;
                    while name_end < self.struct_block.len() && self.struct_block[name_end] != 0 {
                        name_end += 1;
                    }
                    if name_end >= self.struct_block.len() {
                        return None;
                    }
                    if let Ok(name) = core::str::from_utf8(&self.struct_block[name_start..name_end]) {
                        let simple_name = if let Some(at_pos) = name.find('@') {
                            &name[..at_pos]
                        } else {
                            name
                        };

                        if target_depth.is_none() && simple_name == node_name {
                            target_depth = Some(depth);
                        }
                    }
                    depth += 1;
                    pos = align4(name_end + 1);
                }
                FDT_END_NODE => {
                    depth -= 1;
                    if let Some(d) = target_depth {
                        if depth <= d {
                            return None;
                        }
                    }
                }
                FDT_PROP => {
                    if pos + 8 > self.struct_block.len() {
                        return None;
                    }
                    let data_size = unsafe { read_u32(self.struct_block.as_ptr().add(pos)).to_be() } as usize;
                    pos += 4;
                    let name_offset = unsafe { read_u32(self.struct_block.as_ptr().add(pos)).to_be() } as usize;
                    pos += 4;

                    if let Some(d) = target_depth {
                        if depth == d + 1 {
                            if let Some(pname) = self.get_string(name_offset) {
                                if pname == prop_name {
                                    let data_start = pos;
                                    if data_start + data_size > self.struct_block.len() {
                                        return None;
                                    }
                                    return Some(&self.struct_block[data_start..data_start + data_size]);
                                }
                            }
                        }
                    }
                    pos = align4(pos + data_size);
                }
                FDT_NOP => {}
                FDT_END => {
                    return None;
                }
                _ => {
                    return None;
                }
            }
        }
        None
    }
}

/// 从设备树二进制解析硬件信息
pub fn parse(dtb_ptr: *const u8) -> Result<BootInfo, &'static str> {
    if dtb_ptr.is_null() {
        return Err("DTB pointer is null");
    }

    let header = unsafe { FdtHeader::parse(dtb_ptr)? };
    let mut parser = Parser::new(dtb_ptr, &header);

    let mut boot = BootInfo {
        uart_base: DEFAULT_UART_BASE,
        ram_base: DEFAULT_RAM_BASE,
        ram_size: DEFAULT_RAM_SIZE,
    };

    if let Some(reg) = parser.find_node_property("memory", "reg") {
        if reg.len() >= 16 {
            let addr = u64::from_be_bytes([reg[0], reg[1], reg[2], reg[3],
                                           reg[4], reg[5], reg[6], reg[7]]) as usize;
            let size = u64::from_be_bytes([reg[8], reg[9], reg[10], reg[11],
                                           reg[12], reg[13], reg[14], reg[15]]) as usize;
            boot.ram_base = addr;
            boot.ram_size = size;
        }
    }

    if let Some(reg) = parser.find_node_property("pl011", "reg") {
        if reg.len() >= 16 {
            let addr = u64::from_be_bytes([reg[0], reg[1], reg[2], reg[3],
                                           reg[4], reg[5], reg[6], reg[7]]) as usize;
            boot.uart_base = addr;
        }
    }

    Ok(boot)
}
