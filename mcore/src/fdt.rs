use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

pub struct BootInfo {
    pub uart_base: usize,
    pub uart_type: heapless::String<16>,
    pub ram_base: usize,
    pub ram_size: usize,
    /// GICv2 distributor MMIO base.  0 if no GIC was found (in which
    /// case the timer / interrupt driver must not be enabled).
    pub gicd_base: usize,
    /// GICv2 CPU-interface MMIO base.
    pub gicc_base: usize,
}

impl BootInfo {
    pub const fn empty() -> Self {
        BootInfo {
            uart_base: 0,
            uart_type: heapless::String::new(),
            ram_base: 0,
            ram_size: 0,
            gicd_base: 0,
            gicc_base: 0,
        }
    }
}

/// Single CPU core descriptor extracted from `/cpus`.  Mirrors the
/// relevant subset of `cpu@N` properties used by the SMP bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuDescriptor {
    /// 0-based `cpu@N` index (= `reg[0]` in the DTB).
    pub reg: u32,
    /// True if the node carries `status = "okay"` or omits the
    /// `status` property altogether (= "okay" by default per spec).
    pub enabled: bool,
    /// `enable-method` string copied from the property, if present.
    pub enable_method: heapless::String<16>,
    /// `compatible` string copy.  E.g. `"arm,cortex-a72"`.
    pub compatible: heapless::String<32>,
}

impl CpuDescriptor {
    pub const fn empty() -> Self {
        CpuDescriptor {
            reg: 0,
            enabled: false,
            enable_method: heapless::String::new(),
            compatible: heapless::String::new(),
        }
    }
}

/// Result of scanning `/cpus` — at most `MAX_CPUS_IN_DTB` entries
/// returned in `reg`-sorted order.
pub const MAX_CPUS_IN_DTB: usize = 16;

pub struct CpuScanResult {
    pub count: usize,
    pub cpus: [CpuDescriptor; MAX_CPUS_IN_DTB],
}

impl CpuScanResult {
    pub const fn empty() -> Self {
        CpuScanResult {
            count: 0,
            cpus: [const { CpuDescriptor::empty() }; MAX_CPUS_IN_DTB],
        }
    }
}

/// Bit-set bitmask of online CPU slots (post-probe, post-PSCI confirm).
pub struct CpuMask {
    bits: AtomicU64,
}

impl CpuMask {
    pub const fn new() -> Self {
        CpuMask { bits: AtomicU64::new(0) }
    }
    pub fn set(&self, b: usize) {
        self.bits.fetch_or(1u64 << b, Ordering::Relaxed);
    }
    pub fn clear(&self, b: usize) {
        self.bits.fetch_and(!(1u64 << b), Ordering::Relaxed);
    }
    pub fn test(&self, b: usize) -> bool {
        (self.bits.load(Ordering::Relaxed) >> b) & 1 == 1
    }
    pub fn count(&self) -> usize {
        self.bits.load(Ordering::Relaxed).count_ones() as usize
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
        uart_type: heapless::String::new(),
        ram_base: DEFAULT_RAM_BASE,
        ram_size: DEFAULT_RAM_SIZE,
        gicd_base: 0,
        gicc_base: 0,
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

    let mut uart_node = "";
    if let Some(reg) = parser.find_node_property("pl011", "reg") {
        if reg.len() >= 16 {
            boot.uart_base = u64::from_be_bytes([reg[0], reg[1], reg[2], reg[3],
                                                 reg[4], reg[5], reg[6], reg[7]]) as usize;
            uart_node = "pl011";
        }
    } else if let Some(reg) = parser.find_node_property("uart", "reg") {
        if reg.len() >= 16 {
            boot.uart_base = u64::from_be_bytes([reg[0], reg[1], reg[2], reg[3],
                                                 reg[4], reg[5], reg[6], reg[7]]) as usize;
            uart_node = "uart";
        }
    } else if let Some(reg) = parser.find_node_property("serial", "reg") {
        if reg.len() >= 16 {
            boot.uart_base = u64::from_be_bytes([reg[0], reg[1], reg[2], reg[3],
                                                 reg[4], reg[5], reg[6], reg[7]]) as usize;
            uart_node = "serial";
        }
    }

    if !uart_node.is_empty() {
        if let Some(comp_bytes) = parser.find_node_property(uart_node, "compatible") {
            if let Ok(comp_str) = core::str::from_utf8(comp_bytes) {
                if comp_str.contains("pl011") {
                    let _ = boot.uart_type.push_str("pl011");
                } else if comp_str.contains("16550") {
                    let _ = boot.uart_type.push_str("ns16550");
                }
            }
        }
    }

    if boot.uart_type.is_empty() {
        let _ = boot.uart_type.push_str("pl011");
    }

    // ── GICv2 discovery (AArch64 only) ───────────────────────────
    // The QEMU `virt` device tree has a node compatible with
    // "arm,cortex-a15-gic" at /intc with a single `reg` property
    // covering two 4 KiB pages: distributor followed by CPU
    // interface.
    #[cfg(target_arch = "aarch64")]
    {
        if let Some(reg) = parser.find_node_property("intc", "reg") {
            // `reg` is a sequence of (addr_hi, addr_lo, size_hi, size_lo)
            // 32-bit cells.  For GICv2 on QEMU `virt`, address-cells
            // and size-cells are both 2 (inherited from the root node),
            // so each tuple is 16 bytes.  GICv2 has two tuples:
            //   [0..16]  distributor (GICD)
            //   [16..32] CPU interface (GICC)
            if reg.len() >= 32 {
                let gicd_addr = u64::from_be_bytes([
                    reg[0], reg[1], reg[2], reg[3],
                    reg[4], reg[5], reg[6], reg[7],
                ]);
                let gicc_addr = u64::from_be_bytes([
                    reg[16], reg[17], reg[18], reg[19],
                    reg[20], reg[21], reg[22], reg[23],
                ]);
                boot.gicd_base = gicd_addr as usize;
                boot.gicc_base = gicc_addr as usize;
            }
        }
    }

    Ok(boot)
}

/// Walk `/cpus` and emit a `CpuScanResult` describing each child.
///
/// The Linux-style FDT contract: every `cpu@N` child carries
/// `device_type = "cpu"`, a `reg` cell, an optional `enable-method`
/// and an optional `status`.  Anything lacking `device_type =
/// "cpu"` is skipped (this filters out the `cpu-map` / cluster
/// metadata tree — which QEMU and most firmwares use only for
/// hierarchy hints, not as actual cores).
pub fn scan_cpus_node(dtb_ptr: *const u8) -> Result<CpuScanResult, &'static str> {
    if dtb_ptr.is_null() {
        return Err("DTB pointer is null");
    }
    let header = unsafe { FdtHeader::parse(dtb_ptr)? };
    let mut parser = Parser::new(dtb_ptr, &header);

    let mut out = CpuScanResult::empty();

    struct Acc {
        current_reg: u32,
        current_enabled: bool,
        current_method: heapless::String<16>,
        current_compat: heapless::String<32>,
        reg_seen: bool,
        device_type_ok: bool,
        enabled_seen: bool,
    }
    impl Acc {
        fn reset(&mut self) {
            self.current_reg = 0;
            self.current_enabled = true;
            self.current_method.clear();
            self.current_compat.clear();
            self.reg_seen = false;
            self.device_type_ok = false;
            self.enabled_seen = false;
        }
        fn flush_if_valid(&mut self, out: &mut CpuScanResult) {
            if !self.device_type_ok || !self.reg_seen {
                self.reset();
                return;
            }
            if out.count < MAX_CPUS_IN_DTB {
                let idx = out.count;
                out.cpus[idx] = CpuDescriptor {
                    reg: self.current_reg,
                    enabled: self.current_enabled,
                    enable_method: heapless::String::new(),
                    compatible: heapless::String::new(),
                };
                let _ = out.cpus[idx].enable_method.push_str(&self.current_method);
                let _ = out.cpus[idx].compatible.push_str(&self.current_compat);
                out.count += 1;
            }
            self.reset();
        }
    }

    let mut acc = Acc {
        current_reg: 0,
        current_enabled: true,
        current_method: heapless::String::new(),
        current_compat: heapless::String::new(),
        reg_seen: false,
        device_type_ok: false,
        enabled_seen: false,
    };
    acc.reset();

    #[derive(PartialEq)]
    enum State {
        Top,
        InCpus,
    }
    let mut state = State::Top;
    let mut cpus_depth: i32 = -1;
    let mut child_active = false;

    // We track child's begin depth relative to cpus_depth.  When
    // we see a BEGIN_NODE at depth == cpus_depth + 1, we start
    // collecting properties until the matching END_NODE.
    let mut depth: i32 = 0;
    let mut pos = 0usize;
    while pos < parser.struct_block.len() {
        if pos + 4 > parser.struct_block.len() { break; }
        let token = unsafe { read_u32(parser.struct_block.as_ptr().add(pos)).to_be() };
        pos += 4;

        match token {
            FDT_BEGIN_NODE => {
                let name_start = pos;
                let mut name_end = name_start;
                while name_end < parser.struct_block.len()
                    && parser.struct_block[name_end] != 0
                { name_end += 1; }
                if name_end >= parser.struct_block.len() { break; }
                let name = core::str::from_utf8(
                    &parser.struct_block[name_start..name_end]
                ).unwrap_or("");
                let simple = if let Some(at_pos) = name.find('@') { &name[..at_pos] } else { name };

                match state {
                    State::Top if simple == "cpus" => {
                        state = State::InCpus;
                        cpus_depth = depth;
                    }
                    State::InCpus if depth == cpus_depth + 1 => {
                        // New child of /cpus.
                        acc.reset();
                        // Parse the trailing `@<digits>` to extract reg,
                        // preferring the property over the name.
                        if let Some(at_pos) = name.find('@') {
                            let (_, rest) = name.split_at(at_pos + 1);
                            if let Ok(v) = rest.parse::<u32>() {
                                acc.current_reg = v;
                                acc.reg_seen = true;
                            }
                        }
                        child_active = true;
                    }
                    _ => {}
                }

                depth += 1;
                pos = align4(name_end + 1);
            }
            FDT_END_NODE => {
                depth -= 1;
                if state == State::InCpus && child_active
                    && depth == cpus_depth + 1
                {
                    acc.flush_if_valid(&mut out);
                    child_active = false;
                } else if state == State::InCpus && depth == cpus_depth {
                    state = State::Top;
                }
            }
            FDT_PROP => {
                if pos + 8 > parser.struct_block.len() { break; }
                let data_size = unsafe { read_u32(parser.struct_block.as_ptr().add(pos)).to_be() } as usize;
                pos += 4;
                let name_offset = unsafe { read_u32(parser.struct_block.as_ptr().add(pos)).to_be() } as usize;
                pos += 4;
                let pname = parser.get_string(name_offset).unwrap_or("");
                let data_start = pos;
                if data_start + data_size <= parser.struct_block.len() {
                    let data = &parser.struct_block[data_start..data_start + data_size];
                    if state == State::InCpus && child_active
                        && depth == cpus_depth + 2
                    {
                        match pname {
                            "device_type" => {
                                if let Ok(s) = core::str::from_utf8(data) {
                                    let _ = s.trim_end_matches('\0');
                                    if s.trim_end_matches('\0') == "cpu" {
                                        acc.device_type_ok = true;
                                    }
                                }
                            }
                            "reg" => {
                                if data.len() >= 4 {
                                    let v = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                                    acc.current_reg = v;
                                    acc.reg_seen = true;
                                }
                            }
                            "enable-method" => {
                                if let Ok(s) = core::str::from_utf8(data) {
                                    acc.current_method.clear();
                                    let _ = acc.current_method.push_str(s.trim_end_matches('\0'));
                                }
                            }
                            "compatible" => {
                                if let Ok(s) = core::str::from_utf8(data) {
                                    acc.current_compat.clear();
                                    let _ = acc.current_compat.push_str(s.trim_end_matches('\0'));
                                }
                            }
                            "status" => {
                                if let Ok(s) = core::str::from_utf8(data) {
                                    let t = s.trim_end_matches('\0');
                                    acc.current_enabled = t == "okay";
                                    acc.enabled_seen = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                pos = align4(pos + data_size);
            }
            FDT_NOP => {}
            FDT_END => break,
            _ => break,
        }
    }

    Ok(out)
}
