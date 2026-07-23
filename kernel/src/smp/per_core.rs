//! Per-core secondary entry point and slot assignment.
//!
//! `kmain_secondary` is called once per PSCI-woken CPU.  Its job
//! is to:
//!
//!   1. Record this CPU's slot via `register_core()`.
//!   2. Bring up the per-core GIC interface and generic timer.
//!   3. Enable IRQs.
//!   4. Enter the scheduler, which will hand off control to the
//!      first runnable thread on this CPU (or park it in WFE if no
//!      thread is available).
//!
//! The mapping from `MPIDR_EL1.Aff0` to `slot` is established by
//! `kmain_secondary`'s caller in the boot trampoline; we just
//! trust whatever value was passed.

use crate::arch::{ArchHardware, CurrentArch};

/// Native EL1 secondary core entry point.  Writes back the
/// SECONDARY_CORE_ENTRY mailbox to acknowledge the boot, then hands
/// control to `Scheduler::schedule()`.
#[no_mangle]
pub extern "C" fn kmain_secondary(slot: usize) -> ! {
    crate::smp::register_core(slot);

    // Bring up per-core peripherals.  These calls are idempotent
    // w.r.t. the global init done in `kernel_main` for core 0.
    #[cfg(target_arch = "aarch64")]
    {
        crate::drivers::gic::init_local_cpu_interface();
        crate::drivers::timer::init();
        crate::arch::aarch64::trap::enable_irqs();
    }

    crate::log_info!("SMP-SECONDARY", "Core slot {} fully initialized", slot);

    crate::smp::SECONDARY_ENTRY_COUNT
        .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    crate::log_info!(
        "SMP-SECONDARY",
        "slot {} entering scheduler loop (count={})",
        slot,
        crate::smp::SECONDARY_ENTRY_COUNT.load(core::sync::atomic::Ordering::Relaxed)
    );

    loop {
        unsafe {
            crate::task::scheduler::SCHEDULER.schedule();
        }
        // In the unlikely event `schedule()` ever returns (e.g.
        // both online cores are dead and an external event loops
        // us in), park in WFE until the next interrupt.
        unsafe {
            CurrentArch::wait_for_event();
        }
    }
}
