//! # 🏎️ SMP (Symmetric Multiprocessing) Core Booting & Handshake
//!
//! Coordinates multi-core boot sequences on AArch64 using an assembly-level mailbox
//! combined with ARM PSCI CPU_ON firmware calls.
//! Each secondary core is started one by one with a strict acknowledgment handshake
//! to prevent racing and memory write corruption.

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::arch::ArchHardware;

extern "C" {
    static mut SECONDARY_CORE_ENTRY: usize;
    static mut SECONDARY_CORE_SP: usize;
}

/// The total number of active cores in the system (including Core 0).
pub static ACTIVE_CORES: AtomicUsize = AtomicUsize::new(1);

/// Kernel physical entry point for secondary core boot.
const KERNEL_ENTRY_PHYS_ADDR: u64 = 0x40080000;

/// Call ARM PSCI CPU_ON to wake up a specific core at the physical entry point.
#[cfg(target_arch = "aarch64")]
fn psci_cpu_on(cpu_id: u64, entry_point_pa: u64) -> i32 {
    let mut ret: u64;
    unsafe {
        // Try Hypervisor Call (HVC) first, which is standard on non-secure EL1 hypervisors
        core::arch::asm!(
            "hvc #0",
            inout("x0") 0xC4000003u64 => ret,
            in("x1") cpu_id,
            in("x2") entry_point_pa,
            in("x3") 0u64,
            out("x4") _, out("x5") _, out("x6") _, out("x7") _,
        );
    }
    if (ret as i32) < 0 {
        // Fallback to Secure Monitor Call (SMC) if HVC is not supported or returns error
        unsafe {
            core::arch::asm!(
                "smc #0",
                inout("x0") 0xC4000003u64 => ret,
                in("x1") cpu_id,
                in("x2") entry_point_pa,
                in("x3") 0u64,
                out("x4") _, out("x5") _, out("x6") _, out("x7") _,
            );
        }
    }
    ret as i32
}

#[cfg(not(target_arch = "aarch64"))]
fn psci_cpu_on(_cpu_id: u64, _entry_point_pa: u64) -> i32 {
    0
}

/// Boot Core 1, Core 2, and Core 3 using our high-reliability mailbox handshake.
pub fn boot_secondary_cores() {
    crate::log_info!("SMP", "Booting secondary CPU cores...");

    for core_id in 1..4 {
        // Allocate a separate 4KB kernel stack page for each secondary core
        let stack_page_pa = crate::arch::aarch64::phys::alloc_kstack_page()
            .expect("Failed to allocate stack page for secondary core");
        let stack_top_va = crate::arch::mmu_facade::pa_to_kernel_va(stack_page_pa.as_usize()) + 4096;

        crate::log_info!("SMP", "Waking up Core {} (SP Top = {:#x})...", core_id, stack_top_va);

        unsafe {
            // 1. Write the stack top and the secondary kmain entry point into the mailbox
            core::ptr::write_volatile(&mut SECONDARY_CORE_SP as *mut usize, stack_top_va);
            core::ptr::write_volatile(&mut SECONDARY_CORE_ENTRY as *mut usize, kmain_secondary as usize);
            
            // Ensure memory writes are visible before calling PSCI
            core::sync::atomic::fence(Ordering::SeqCst);

            // 2. Invoke PSCI CPU_ON to wake up the secondary core at _start (physical 0x40080000)
            let psci_ret = psci_cpu_on(core_id as u64, KERNEL_ENTRY_PHYS_ADDR);
            if psci_ret != 0 {
                crate::log_error!("SMP", "PSCI CPU_ON failed for Core {} with error code: {}", core_id, psci_ret);
                continue;
            }

            // 3. Spin-wait for the secondary core to clear ENTRY to 0 to acknowledge boot
            while core::ptr::read_volatile(&SECONDARY_CORE_ENTRY as *const usize) != 0 {
                core::hint::spin_loop();
            }
        }

        // Increment total active cores in the system
        ACTIVE_CORES.fetch_add(1, Ordering::SeqCst);
        crate::log_info!("SMP", "Core {} booted successfully and acknowledged!", core_id);
    }

    crate::log_info!("SMP", "All secondary cores active! Total Cores = {}", ACTIVE_CORES.load(Ordering::Relaxed));
}

/// Native EL1 secondary core entry point.
#[no_mangle]
pub extern "C" fn kmain_secondary(core_id: usize) -> ! {
    // 1. Initialize local CPU interrupt interface and generic timer
    crate::drivers::gic::init_local_cpu_interface();
    crate::drivers::timer::init();

    crate::log_info!("SMP-SECONDARY", "Core {} fully initialized, enabling local IRQs...", core_id);

    // 2. Enable local CPU interrupts
    crate::arch::aarch64::trap::enable_irqs();

    // 3. Enter the preemptive multitasking loop
    loop {
        unsafe {
            crate::task::scheduler::SCHEDULER.schedule();
        }
        // If there are no threads ready to execute, sleep in low-power state
        unsafe {
            crate::arch::CurrentArch::wait_for_event();
        }
    }
}
