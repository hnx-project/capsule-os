pub struct AArch64Cpu;

impl hal::cpu::CpuInfo for AArch64Cpu {
    fn id() -> u32 { 0 }
    fn count() -> u32 { 1 }
    fn features() -> hal::cpu::CpuFeatures {
        hal::cpu::CpuFeatures {
            has_fp: true, has_simd: true, has_crc32: true,
            has_aes: true, has_pmull: true, has_sha1: true, has_sha2: true,
        }
    }
}

impl hal::cpu::Cpu for AArch64Cpu {
    fn enable_mmu() {}
    fn enable_irq() {}
    fn disable_irq() {}
    fn yield_() {}
    fn current_el() -> u32 { 0 }
    fn set_stack_pointer(_sp: usize) {}
    fn get_stack_pointer() -> usize { 0 }
    fn flush_tlb() {}
    fn is_in_kernel() -> bool { true }
}
