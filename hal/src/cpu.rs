use shared::Status;

#[derive(Debug, Clone, Copy)]
pub struct CpuFeatures {
    pub has_fp: bool,
    pub has_simd: bool,
    pub has_crc32: bool,
    pub has_aes: bool,
    pub has_pmull: bool,
    pub has_sha1: bool,
    pub has_sha2: bool,
}

pub trait CpuInfo: Send + Sync {
    fn id() -> u32;
    fn count() -> u32;
    fn features() -> CpuFeatures;
}

pub trait Cpu: CpuInfo {
    fn enable_mmu();
    fn enable_irq();
    fn disable_irq();
    fn yield_();
    fn current_el() -> u32;
    fn set_stack_pointer(sp: usize);
    fn get_stack_pointer() -> usize;
    fn flush_tlb();
    fn is_in_kernel() -> bool;
}
