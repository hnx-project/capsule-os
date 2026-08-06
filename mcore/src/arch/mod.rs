pub mod console;
pub mod mmu_facade;

pub mod aarch64;

/// 故障或 Panic 时的 CPU 特权级寄存器诊断数据包。
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuDiagnostics {
    pub pc: usize,
    pub sp: usize,
    pub spsr_or_status: usize,
    pub elr_or_epc: usize,
    pub esr_or_cause: usize,
}

/// 架构特异性线程寄存器上下文契约。
/// 所有的硬件平台相关的 TrapFrame / Context 寄存器存储均实现此 Trait。
pub trait ArchContext: Sized + Default + Clone + Copy + Send + Sync {
    /// 创建一个初始的内核态执行上下文。
    fn new_kernel(entry: usize, stack_top: usize) -> Self;

    /// 创建一个初始的用户态执行上下文。
    fn new_user(entry: usize, stack_top: usize, kernel_stack_top: usize) -> Self;

    /// 设置此上下文专用的进程页表和 PID 信息。
    fn set_page_table(&mut self, process_id: u64, l0_user_pa: u64);
}

/// 架构特异性物理页表树管理契约。
pub trait ArchPageTable: Send + Sync + core::fmt::Debug {
    /// 创建一个未分配物理根页表的空壳实例
    fn new() -> Self;

    /// 动态分配物理 L0 根页表并构建系统默认映射
    fn allocate_root(&mut self) -> shared::status::Result<()>;

    /// 手动设置根页表基址
    fn set_root_raw(&mut self, l0_pa: usize);

    /// 检测当前页表是否已经分配并持有物理根页表
    fn has_root(&self) -> bool;

    /// 获取 L0 物理基址
    fn l0_pa(&self) -> usize;

    /// 注册由 MMU 缺页引发分配的底层二级、三级物理页表页
    fn track(&mut self, pa: usize);

    /// 验证页表树的物理合法性
    fn validate(&self) -> bool;

    /// 获取当前页表由于更新产生的新增序列戳 (Generation Stamp)
    fn generation(&self) -> u64;

    /// 设置更新戳
    fn set_generation(&mut self, gen: u64);

    /// 销毁并深度物理回收整棵用户态二级、三级页表物理页框
    unsafe fn free_tree(&mut self);

    /// 映射单个用户虚拟页面到物理页框
    fn map_va(&mut self, va: usize, pa: usize, flags: &crate::arch::mmu::MapFlags) -> shared::status::Result<()>;

    /// 翻译用户虚拟地址到物理地址
    fn translate_va(&self, va: usize) -> Option<usize>;

    /// 从当前活动 L0 物理页表中复制所有内核高半区只读/特权映射段 (内核高半区镜像)
    fn clone_high_half(&mut self, src_l0_pa: usize);

    /// 从当前活动 L0 物理页表中复制底层 I/O 设备或早期恒等段
    fn clone_identity_block(&mut self, src_l0_pa: usize);

    /// S3 fork: copy every user-space L0..L3 mapping from `src_l0_pa`
    /// into `self`, allocating fresh sub-table pages for the
    /// caller.  Physical page frames are shared (shallow clone);
    /// see `arch/aarch64/page_table::clone_user_from` for the
    /// trade-offs and future COW plan.
    fn clone_user_from(&mut self, src_l0_pa: usize);
}

/// 架构底盘硬件控制契约。
pub trait ArchHardware {
    type Context: ArchContext;
    type PageTable: ArchPageTable;
    type AddressSpaceId: Copy + Default + PartialEq + Eq + Send + Sync + core::fmt::Debug;

    /// 动态分配 ASID 资源
    fn alloc_asid() -> Option<Self::AddressSpaceId>;

    /// 释放 ASID 资源
    fn free_asid(asid: Self::AddressSpaceId);

    /// 获取内核默认的 ASID (一般为 0)
    fn kernel_asid() -> Self::AddressSpaceId;

    /// 打包页表基址和 ASID 为硬件寄存器期望的写入字
    fn pack_ttbr(l0_pa: u64, asid: Self::AddressSpaceId) -> u64;

    /// 获取当前的 Rollover 周期代数
    fn rollover_generation() -> u32;

    /// 同步高速指令缓存 (Instruction Cache)
    unsafe fn sync_instruction_cache(kva: usize, len: usize);

    /// 强制全局刷新并同步高速指令缓存 (ic ialluis + dsb sy + isb)
    unsafe fn invalidate_instruction_cache();

    /// 执行两线程上下文之间的寄存器硬件切换（switch_to）。
    unsafe fn switch_context(current: *mut Self::Context, next: *const Self::Context);

    /// 安全挂起 CPU 等待下一次硬件中断 (WFI)。
    unsafe fn wait_for_interrupt();

    /// 安全挂起 CPU 等待特定的事件 (WFE)。
    unsafe fn wait_for_event();

    /// 全局刷写或使能当前地址空间的 TLB 缓存。
    unsafe fn flush_tlb();

    /// 安装或恢复用户态页表基址与 ASID
    unsafe fn restore_user_page_table(old_val: usize, new_l0_pa: usize, new_asid: Self::AddressSpaceId);

    /// 清理并写回指定虚拟内存段的高速缓存（Clean / CVAC）。
    unsafe fn clean_cache_range(kva: usize, len: usize);

    /// 清理、写合并并废弃特定虚拟内存段的高速缓存（Clean & Invalidate / CIVAC）。
    unsafe fn clean_and_invalidate_cache_range(kva: usize, len: usize);

    /// 强制使用 DC IVAC & IC IVAU 废弃和冲刷指定的 CPU 栈范围，使其对 CPU 取指与读取完全物理可见。
    unsafe fn invalidate_stack_line(sp_va: usize);

    /// 备份并禁用当前 CPU 中断，返回备份的状态字 (AArch64 DAIF / RISCV sstatus)
    unsafe fn local_irq_save() -> usize;

    /// 恢复之前备份的 CPU 中断状态
    unsafe fn local_irq_restore(flags: usize);

    /// 触发全数据同步和内存访问屏障 (DSB ISH)。
    unsafe fn memory_barrier();

    /// 触发指令缓存同步与流水线刷新屏障 (ISB)。
    unsafe fn instruction_barrier();

    /// 获取当前的运行时调用栈 PC 和 SP。
    fn get_current_registers() -> (usize, usize);

    /// 获取详细的 CPU 特权级核心控制字诊断数据。
    fn get_diagnostics() -> CpuDiagnostics;

    /// 读取当前活动进程的页表 L0 物理基质地址 (TTBR0_EL1)。
    fn get_active_page_table() -> usize;

    /// 获取底层硬件计数器 ticks 读数。
    fn get_hardware_ticks() -> u64;

    /// 设置硬件定时器倒计时比较器的 ticks 偏移。
    fn set_timer_ticks(ticks: u32);

    /// 启用或配置定时器中断。
    fn enable_timer(frequency: usize);
}

/// 全局特异性的硬件底盘实现。
#[cfg(target_arch = "aarch64")]
pub type CurrentArch = aarch64::Aarch64Hardware;
#[cfg(not(target_arch = "aarch64"))]
pub type CurrentArch = DummyHardware;

/// 当指定平台未提供实现或需要空桩时的后备底盘。
#[derive(Default, Clone, Copy, Debug)]
pub struct DummyPageTable;

impl ArchPageTable for DummyPageTable {
    fn new() -> Self { Self }
    fn allocate_root(&mut self) -> shared::status::Result<()> { Ok(()) }
    fn set_root_raw(&mut self, _l0_pa: usize) {}
    fn has_root(&self) -> bool { false }
    fn l0_pa(&self) -> usize { 0 }
    fn track(&mut self, _pa: usize) {}
    fn validate(&self) -> bool { true }
    fn generation(&self) -> u64 { 0 }
    fn set_generation(&mut self, _gen: u64) {}
    unsafe fn free_tree(&mut self) {}
    fn map_va(&mut self, _va: usize, _pa: usize, _flags: &crate::arch::mmu::MapFlags) -> shared::status::Result<()> { Ok(()) }
    fn translate_va(&self, _va: usize) -> Option<usize> { None }
    fn clone_high_half(&mut self, _src_l0_pa: usize) {}
    fn clone_identity_block(&mut self, _src_l0_pa: usize) {}
    fn clone_user_from(&mut self, _src_l0_pa: usize) {}
}

#[derive(Default, Clone, Copy)]
pub struct DummyContext;

impl ArchContext for DummyContext {
    fn new_kernel(_entry: usize, _stack_top: usize) -> Self { Self }
    fn new_user(_entry: usize, _stack_top: usize, _kernel_stack_top: usize) -> Self { Self }
    fn set_page_table(&mut self, _process_id: u64, _l0_user_pa: u64) {}
}

pub struct DummyHardware;

impl ArchHardware for DummyHardware {
    type Context = DummyContext;
    type PageTable = DummyPageTable;
    type AddressSpaceId = u16;

    fn alloc_asid() -> Option<Self::AddressSpaceId> { Some(0) }
    fn free_asid(_asid: Self::AddressSpaceId) {}
    fn kernel_asid() -> Self::AddressSpaceId { 0 }
    fn pack_ttbr(_l0_pa: u64, _asid: Self::AddressSpaceId) -> u64 { 0 }
    fn rollover_generation() -> u32 { 0 }
    unsafe fn sync_instruction_cache(_kva: usize, _len: usize) {}
    unsafe fn invalidate_instruction_cache() {}

    unsafe fn switch_context(_current: *mut Self::Context, _next: *const Self::Context) {}
    unsafe fn wait_for_interrupt() {}
    unsafe fn wait_for_event() {}
    unsafe fn flush_tlb() {}
    unsafe fn restore_user_page_table(_old_val: usize, _new_l0_pa: usize, _new_asid: Self::AddressSpaceId) {}
    unsafe fn clean_cache_range(_kva: usize, _len: usize) {}
    unsafe fn clean_and_invalidate_cache_range(_kva: usize, _len: usize) {}
    unsafe fn invalidate_stack_line(_sp_va: usize) {}
    unsafe fn local_irq_save() -> usize { 0 }
    unsafe fn local_irq_restore(_flags: usize) {}
    unsafe fn memory_barrier() {}
    unsafe fn instruction_barrier() {}
    fn get_current_registers() -> (usize, usize) { (0, 0) }
    fn get_diagnostics() -> CpuDiagnostics { CpuDiagnostics::default() }
    fn get_active_page_table() -> usize { 0 }
    fn get_hardware_ticks() -> u64 { 0 }
    fn set_timer_ticks(_ticks: u32) {}
    fn enable_timer(_frequency: usize) {}
}

pub fn early_init() {
    #[cfg(target_arch = "aarch64")]
    aarch64::early_init();
}

pub fn console_putchar(c: u8) {
    crate::drivers::uart::putchar(c);
}

pub fn console_getchar() -> Option<u8> {
    crate::drivers::uart::getchar()
}

pub fn console_putbytes(s: &[u8]) {
    for &c in s {
        console_putchar(c);
    }
}

#[cfg(target_arch = "aarch64")]
pub use aarch64::trap;

#[cfg(target_arch = "aarch64")]
pub use aarch64::mmu::translate_user_va;

#[cfg(target_arch = "aarch64")]
pub mod phys {
    pub use crate::arch::aarch64::phys::*;
}
#[cfg(target_arch = "aarch64")]
pub mod mmu {
    pub use crate::arch::aarch64::mmu::*;
}
#[cfg(target_arch = "aarch64")]
pub mod slab {
    pub use crate::arch::aarch64::slab::*;
}
