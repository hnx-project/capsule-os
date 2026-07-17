//! Cross-platform generic timer driver.
//!
//! Handles AArch64 EL1 physical counter timer and RISC-V 64 S-Mode timer via OpenSBI.

use crate::arch::{ArchHardware, CurrentArch};

pub const TIMER_INTERVAL_TICKS: u64 = 1_000_000; // ~16 ms at 62.5 MHz QEMU default

static mut TICK_COUNT: u64 = 0;

/// Initialize the architecture-specific timer.
pub fn init() {
    CurrentArch::enable_timer(TIMER_INTERVAL_TICKS as usize);
    CurrentArch::set_timer_ticks(TIMER_INTERVAL_TICKS as u32);
}

/// Read the currently-programmed tick interval (in counter ticks).
pub fn interval_ticks() -> u64 {
    TIMER_INTERVAL_TICKS
}

/// Read the timer frequency (Hz).
pub fn freq_hz() -> u64 {
    // 62.5 MHz standard default
    62_500_000
}

/// Read the 64-bit physical counter.
pub fn phys_count() -> u64 {
    CurrentArch::get_hardware_ticks()
}

pub fn get_ticks() -> u64 {
    unsafe { TICK_COUNT }
}

/// Called from the IRQ dispatcher (IRQ-EL0 / IRQ-EL1 paths) on every timer
/// tick.
#[cfg(target_arch = "aarch64")]
pub fn handle_tick_from_irq(frame: *mut crate::arch::aarch64::trap::TrapFrame) {
    unsafe {
        TICK_COUNT += 1;

        // Re-arm timer first
        CurrentArch::set_timer_ticks(TIMER_INTERVAL_TICKS as u32);

        // Fetch ELR and SPSR using current generic arch diagnostics
        let diag = CurrentArch::get_diagnostics();
        let elr = diag.elr_or_epc as u64;
        let spsr = diag.spsr_or_status as u64;

        let from_el0 = (spsr & 0xF) == 0;
        if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
            // **Only snapshot elr/spsr/x into the thread context when the
            // IRQ came from EL0.**
            if from_el0 {
                (*t).context.elr = elr;
                (*t).context.spsr = spsr;
                if !frame.is_null() {
                    let f = &*frame;
                    (*t).context.x = f.x;
                }
            }
        }

        // Trigger the preemptive scheduler
        crate::task::scheduler::SCHEDULER.schedule();

        // TTBR0 hardening: reload page table
        if from_el0 {
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                let pid = (*t).process_id;
                if let Some((l0_pa, asid)) = crate::task::process::find_process_l0_user_pa(pid) {
                    crate::arch::aarch64::mmu::set_ttbr0_el1(l0_pa, asid);
                }
            }
        }
    }
}

/// Backwards-compatible alias for IRQ paths that don't pass a frame.
#[cfg(not(target_arch = "aarch64"))]
pub fn handle_tick_from_irq(_frame: *mut ()) {
    handle_tick();
}

pub fn handle_tick() {
    unsafe {
        TICK_COUNT += 1;

        // Re-arm timer first
        CurrentArch::set_timer_ticks(TIMER_INTERVAL_TICKS as u32);

        // Trigger the preemptive scheduler (no frame persistence here).
        crate::task::scheduler::SCHEDULER.schedule();
    }
}
