//! AArch64 EL1 exception (trap) handling.
//!
//! ## Layout
//!
//! `boot_asm.S` installs `vector_table` at `vbar_el1`.  Each of the
//! 16 vector slots is a branch into one of the per-class Rust
//! entry stubs.  Only the **IRQ-EL1-SP_ELx** slot is fully wired
//! up; the rest currently halt after a diagnostic print on the
//! UART, so a wrong-class trap stays visible in QEMU output
//! instead of disappearing into a spin loop.
//!
//! ## Frame layout
//!
//! The IRQ stub builds a `TrapFrame` on the trap stack:
//!
//! ```text
//!   offset  size  field
//!   0x000   0x98  x[0..=18]                 (152 bytes, 19 u64s)
//!   0x098   0x08  spsr_el1
//!   0x0A0   0x08  elr_el1
//!   0x0A8   0x08  esr_el1
//!   0x0B0   0x08  far_el1
//! ```
//!
//! 19 registers because x0..x18 are caller-saved in the AAPCS
//! and the trap stub has to preserve them across the dispatcher
//! call.  x19..x28 are callee-saved (so the kernel never crosses
//! a trap boundary with live values in them) and x29/x30 belong
//! to the dispatcher itself.
//!
//! ## Locking
//!
//! Trap handlers run with kernel IRQs still masked (the stub
//! leaves `PSTATE.I` set during frame save/restore).  A future
//! scheduler tick may re-enable IRQs at the bottom of the call;
//! for now we service and `eret` with IRQs still masked, which
//! is fine on a single-CPU bring-up.

/// Called from the IRQ-EL1-SP_ELx assembly stub after the GIC has
/// been acknowledged.  The IAR value identifies which interrupt
/// fired: PPI #30 (INTID 30) is the generic timer.
#[no_mangle]
pub extern "C" fn irq_handler(iar: u32, frame: *mut TrapFrame) {
    // PPI #30 (INTID 30) = generic timer.
    if iar == 30 {
        crate::drivers::timer::handle_tick_from_irq(frame);
    }
    // Other INTIDs are silently EOId (the stub always EOIs).
}

/// Snapshot of the interrupted register state.
/// Kept for future use by the trap dispatcher (sync/SError handlers).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    pub x: [u64; 19],
    pub spsr: u64,
    pub elr: u64,
    pub esr: u64,
    pub far: u64,
    pub lr: u64,
}

#[no_mangle]
pub extern "C" fn aarch64_sync_el0_handler(frame: *mut TrapFrame) {
    let esr = unsafe { (*frame).esr };
    let ec = (esr >> 26) & 0x3F; // Exception Class

    if ec == 0x11 || ec == 0x15 {
        // SVC exceptions in AArch64/AArch32 state
        // EC=0x11: SVC in AArch64
        // EC=0x15: SVC in AArch32 (or trapped MSR/MRS)
        unsafe {
            let syscall_num = (*frame).x[16];

            // One-line visibility for the EXEC syscall (#110); every other
            // syscall is silent and is handled by the dispatch table.
            if syscall_num == 110 {
                crate::log_info!(
                    "SYSCALL",
                    "EXEC x0={:#x} x1={:#x} ELR={:#x}",
                    (*frame).x[0],
                    (*frame).x[1],
                    (*frame).elr,
                );
            }

            let ret = crate::syscall::syscall_dispatch(
                syscall_num as u32,
                (*frame).x[0] as usize,
                (*frame).x[1] as usize,
                (*frame).x[2] as usize,
                (*frame).x[3] as usize,
                (*frame).x[4] as usize,
                (*frame).x[5] as usize,
            );

            (*frame).x[0] = ret as u64; // Return value in x0
            // SVC exception records ELR at the SVC instruction; advance past
            // it so eret resumes at the instruction after svc #0.
            (*frame).elr += 4;
            // Stash the (now-advanced) ELR into the current thread's
            // ThreadContext.  The IRQ path also writes elr on every tick,
            // but updating here makes sure that a syscall that immediately
            // causes a context switch (or a syscall return that is then
            // preempted) still has the right resume PC.
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_mut() {
                t.context.elr = (*frame).elr;
            }
        }
    } else {
        // Non-SVC EL0 exception (data abort, instruction abort, etc.)
        // Log details, kill the faulting user-space thread, and trigger rescheduling to avoid dead EL0 infinite loops!
        unsafe {
            let far = (*frame).far;
            let elr = (*frame).elr;
            let spsr = (*frame).spsr;
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_mut() {
                crate::log_error!(
                    "EL0-FAULT",
                    "EC={:#x} ESR={:#x} ELR={:#x} FAR={:#x} SPSR={:#x} thread=#{} -- KILLED thread to prevent looping exception",
                    ec, esr, elr, far, spsr, t.id
                );
                t.state = crate::task::thread::ThreadState::Dead;
                crate::task::scheduler::SCHEDULER.schedule();
            } else {
                crate::log_error!(
                    "EL0-FAULT",
                    "EC={:#x} ESR={:#x} ELR={:#x} FAR={:#x} SPSR={:#x}",
                    ec, esr, elr, far, spsr
                );
            }
        }
    }
}

/// Last-resort handler for traps we don't yet service.  Logs
/// ESR / ELR / FAR and spins.
#[cold]
#[inline(never)]
fn panic_unhandled(frame: &TrapFrame, class: u64) {
    let iss = frame.esr & 0x1FF_FFFF;
    panic!(
        "Unhandled exception: class={:#x}, esr={:#x}, iss={:#x}, elr={:#x}, far={:#x}",
        class, frame.esr, iss, frame.elr, frame.far
    );
}

/// Mask IRQs at the CPU level (set `PSTATE.I`).
#[inline]
pub fn disable_irqs() {
    unsafe { core::arch::asm!("msr daifset, #1", options(nomem, preserves_flags)) }
}

/// Unmask IRQs at the CPU level (clear PSTATE.I).
/// Empirical: `daifclr` immediate bit 0 → F, bit 1 → I,
/// bit 2 → D, bit 3 → A.  So #2 clears I.
#[inline]
pub fn enable_irqs() {
    unsafe { core::arch::asm!("msr daifclr, #2", "isb", options(nomem, preserves_flags)) }
}
