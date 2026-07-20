use core::arch::global_asm;

// 导入汇编启动入口
global_asm!(include_str!("entry.S"));

/// 挂起当前 CPU 核心
pub fn halt() -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
