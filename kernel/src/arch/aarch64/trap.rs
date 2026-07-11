//! AArch64 EL1 exception (trap) handling.
//!
//! ## Vector table
//!
//! `boot_asm.S` installs `vector_table` at `vbar_el1`.  Each of the 16
//! vector slots is a branch into one of the per-class Rust entry stubs.
//! The three fully-wired paths are `sync_el0` (SVC dispatch from EL0),
//! `irq_el0` (timer IRQ while in user mode), and `irq_el1_spx` (timer
//! IRQ while in kernel mode).  The other branches halt after a
//! diagnostic print so an unexpected trap class stays visible in QEMU
//! output instead of disappearing into a spin loop.
//!
//! ## Trap frame layout (208 bytes on the stack)
//!
//! The asm stubs build a `TrapFrame` on the trap stack and pass it to
//! the Rust dispatcher:
//!
//! ```text
//!   offset  size  field
//!   0x000   0x98  x[0..=18]            (152 bytes, 19 u64s)
//!   0x098   0x08  spsr_el1
//!   0x0A0   0x08  elr_el1
//!   0x0A8   0x08  esr_el1
//!   0x0B0   0x08  far_el1
//!   0x0B8   0x08  lr (x30)
//!   0x0C0   0x08  sp_el0               (user-mode sp)
//!   0x0C8   0x08  padding (16-byte align)
//! ```
//!
//! x0..x18 are caller-saved in AAPCS, so the stub preserves them
//! across the dispatcher call.  x19..x28 are callee-saved and the
//! dispatcher must preserve them; in particular x19 is reused as
//! the TrapFrame pointer (see `X19Guard` below) and **must not** be
//! clobbered by Rust.
//!
//! ## Locking
//!
//! Trap handlers run with kernel IRQs still masked (the stub leaves
//! `PSTATE.I` set during frame save/restore).  The timer tick path
//! (`handle_tick_from_irq`) re-enables preemption via `SCHEDULER.schedule()`
//! but the trap stubs themselves complete the `eret` with IRQs still
//! masked, which is fine on a single-CPU bring-up.

/// Called from the IRQ-EL1-SP_ELx assembly stub after the GIC has
/// been acknowledged.  The IAR value identifies which interrupt
/// fired: PPI #30 (INTID 30) is the generic timer.
#[no_mangle]
pub extern "C" fn irq_handler(iar: u32, frame: *mut TrapFrame) {
    // **AAPCS workaround**: see `X19Guard` and `aarch64_sync_el0_handler`.
    // The trap asm uses x19 as the TrapFrame pointer; without explicit
    // save/restore the Rust body of this function (which doesn't read
    // x19) leaves x19 clobbered and the IRQ-EL0 / IRQ-EL1-spx epilogue
    // `ldp x0, x1, [x19]` reads from a garbage address.
    let saved_x19: u64;
    unsafe {
        core::arch::asm!(
            "mov {0}, x19",
            out(reg) saved_x19,
            options(nomem, preserves_flags)
        );
    }
    let _x19_guard = X19Guard { saved: saved_x19 };
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
    pub sp: u64,
}

/// RAII guard that restores the AArch64 `x19` register on drop.
/// Used by the trap handlers to honour the AAPCS promise that x19
/// (used as the TrapFrame pointer by the assembly stubs) is callee-
/// saved, even when the Rust function body itself doesn't reference
/// it and the compiler would otherwise leave it clobbered.
struct X19Guard {
    saved: u64,
}

impl Drop for X19Guard {
    fn drop(&mut self) {
        unsafe {
            core::arch::asm!(
                "mov x19, {0}",
                in(reg) self.saved,
                options(nomem, preserves_flags)
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn aarch64_sync_el0_handler(frame: *mut TrapFrame) {
    // **AAPCS workaround**: the trap asm uses x19 as the TrapFrame pointer
    // because x19 is the only callee-saved register that survives a nested
    // IRQ + switch_to (which mutates sp).  Rust's C-ABI does NOT save
    // x19-x28 unless the function body actually uses them, so save x19
    // manually here and restore it on every return path via the
    // `X19Guard` RAII helper.
    let saved_x19: u64;
    unsafe {
        core::arch::asm!(
            "mov {0}, x19",
            out(reg) saved_x19,
            options(nomem, preserves_flags)
        );
    }
    let _x19_guard = X19Guard { saved: saved_x19 };
    let esr = unsafe { (*frame).esr };
    let elr = unsafe { (*frame).elr };
    let spsr = unsafe { (*frame).spsr };
    crate::log_debug!("TRAP", " EL0 Trap Intercepted! ESR={:#x}, ELR={:#x}, SPSR={:#x}", esr, elr, spsr);
    let ec = (esr >> 26) & 0x3F; // Exception Class

    if ec == 0x11 || ec == 0x15 {
        // SVC exceptions in AArch64/AArch32 state
        // EC=0x11: SVC in AArch64
        // EC=0x15: SVC in AArch32 (or trapped MSR/MRS)
        crate::log_debug!(
            "SVC-PRE",
            "n=#{} ec={:#x} elr={:#x}",
            unsafe { (*frame).x[16] }, ec, elr
        );
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
            (*frame).elr += 4;
            // After SVC dispatch, advance ELR by 4 to skip the SVC itself
            // (it is a 4-byte instruction).  Even though QEMU (cortex-a72)
            // reports our AArch64 `svc #0` with ESR.EC=0x15 (a classification
            // quirk on this model — real hardware reports EC=0x11 for
            // AArch64 SVC), the ELR_EL1 value it stores on trap entry is
            // the SVC PC.  Without `+= 4` the eret jumps straight back
            // into the same `svc #0`, the loader's T11+ tracepoints stop
            // firing, and the user-mode program appears to hang.
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                unsafe { (*t).context.elr = (*frame).elr; }
            }
        }
    } else {
        // Non-SVC EL0 exception (data abort, instruction abort, etc.)
        // Log details, kill the faulting user-space thread, and trigger rescheduling to avoid dead EL0 infinite loops!
        unsafe {
            let far = (*frame).far;
            let elr = (*frame).elr;
            let spsr = (*frame).spsr;
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                let far = unsafe { (*frame).far };
                crate::log_error!(
                    "EL0-FAULT",
                    "EC={:#x} ESR={:#x} ELR={:#x} FAR={:#x} SPSR={:#x} thread=#{} -- KILLED thread to prevent looping exception",
                    ec, esr, elr, far, spsr, (*t).id
                );
                (*t).state = crate::task::thread::ThreadState::Dead;
                crate::task::scheduler::SCHEDULER.schedule();
            } else {
                let far = unsafe { (*frame).far };
                crate::log_error!(
                    "EL0-FAULT",
                    "EC={:#x} ESR={:#x} ELR={:#x} FAR={:#x} SPSR={:#x}",
                    ec, esr, elr, far, spsr
                );
            }
        }
    }
}

/// AArch64 SError interrupt handler for EL0 (vector slot 0x580 / 0x780).
///
/// SError is an **asynchronous** external abort — its ESR_EL1.EC value
/// is 0x2F, and the captured ELR_EL1 does not point at the faulting
/// instruction (the fault is not associated with a specific load /
/// store).  Common sources on this platform are bus parity errors,
/// asynchronous external aborts from devices, or MMU walks that
/// faulted in the background.
///
/// Behavioural contract (mirrors the non-SVC branch of
/// `aarch64_sync_el0_handler`):
///   1. Log ESR / ELR / FAR / SPSR with an `SError-FAULT` tag so the
///      caller can attribute the fault to the SError class.
///   2. Mark the current user thread `Dead` so the scheduler will
///      not `eret` back into it.
///   3. Call `SCHEDULER.schedule()` so the system swaps to the next
///      runnable thread.  When no other thread is alive the scheduler
///      will halt the CPU via its "all dead" branch.
#[no_mangle]
pub extern "C" fn aarch64_serror_el0_handler(frame: *mut TrapFrame) {
    // Same x19 callee-saved dance as the sync handler — see X19Guard.
    let saved_x19: u64;
    unsafe {
        core::arch::asm!(
            "mov {0}, x19",
            out(reg) saved_x19,
            options(nomem, preserves_flags)
        );
    }
    let _x19_guard = X19Guard { saved: saved_x19 };

    unsafe {
        let esr = (*frame).esr;
        let elr = (*frame).elr;
        let far = (*frame).far;
        let spsr = (*frame).spsr;
        let ec = (esr >> 26) & 0x3F;
        let iss = esr & 0x1FF_FFFF;
        // AET lives in ESR_EL1.ISS[12:10] for SError.  Decode it so the
        // log distinguishes "uncategorised" (0b000) from
        // "uncontainable" (0b001) — the latter typically signals a
        // poisoned data read that the kernel cannot recover from.
        let aet = (iss >> 10) & 0x7;

        if let Some(t) =
            crate::task::scheduler::SCHEDULER.get_current_thread_ptr()
        {
            crate::log_error!(
                "SError-FAULT",
                "EC={:#x} ESR={:#x} ISS={:#x} AET={:#x} ELR={:#x} FAR={:#x} SPSR={:#x} thread=#{} -- KILLED thread to prevent looping exception",
                ec, esr, iss, aet, elr, far, spsr, (*t).id
            );
            (*t).state = crate::task::thread::ThreadState::Dead;
            crate::task::scheduler::SCHEDULER.schedule();
        } else {
            crate::log_error!(
                "SError-FAULT",
                "EC={:#x} ESR={:#x} ISS={:#x} AET={:#x} ELR={:#x} FAR={:#x} SPSR={:#x} (no current thread)",
                ec, esr, iss, aet, elr, far, spsr
            );
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
    unsafe { core::arch::asm!("msr daifset, #2", "isb", options(nomem, preserves_flags)) }
}

/// Unmask IRQs at the CPU level (clear PSTATE.I).
/// Empirical: `daifclr` immediate bit 0 → F, bit 1 → I,
/// bit 2 → D, bit 3 → A.  So #2 clears I.
#[inline]
pub fn enable_irqs() {
    unsafe { core::arch::asm!("msr daifclr, #2", "isb", options(nomem, preserves_flags)) }
}
