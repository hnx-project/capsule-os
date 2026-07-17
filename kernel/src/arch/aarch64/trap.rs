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

/// Snapshot of the interrupted register state.  The asm trap stubs
/// allocate 208 bytes on the trap stack and fill this struct in the
/// order documented in the module-level docstring; the offsets below
/// are linked 1:1 to the asm frame layout.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    pub x: [u64; 19],   // 0x000
    pub spsr: u64,      // 0x098
    pub elr: u64,       // 0x0A0
    pub esr: u64,       // 0x0A8
    pub far: u64,       // 0x0B0
    pub lr: u64,        // 0x0B8
    pub sp: u64,        // 0x0C0 — user-mode sp (sp_el0 snapshot)
}

/// RAII guard that snapshots `x19` on construction and restores it on
/// drop.  Required because the asm trap stubs stash the TrapFrame
/// pointer in `x19` (so it survives a nested IRQ + switch_to that
/// mutates `sp`), but Rust's `extern "C"` ABI only preserves
/// `x19-x28` when the function body actually uses them — and our
/// trap handler bodies do not.  Without this guard, the asm epilogue
/// `ldp x0, x1, [x19]` reads from a garbage address and the kernel
/// dies on the very next eret.
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
    let esr = unsafe { (*frame).esr };
    let elr = unsafe { (*frame).elr };
    let spsr = unsafe { (*frame).spsr };
    let ec = (esr >> 26) & 0x3F;

    if ec == 0x11 || ec == 0x15 {
        unsafe {
            let syscall_num = (*frame).x[16];

            let ret = crate::syscall::syscall_dispatch(
                syscall_num as u32,
                (*frame).x[0] as usize,
                (*frame).x[1] as usize,
                (*frame).x[2] as usize,
                (*frame).x[3] as usize,
                (*frame).x[4] as usize,
                (*frame).x[5] as usize,
            );

            (*frame).elr += 4;
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                unsafe { (*t).context.elr = (*frame).elr; }
            }

            // TTBR0 reload — do this BEFORE the x0 write so any
            // schedule() side effect can't clobber the return value.
            if let Some(t_svc) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                if let Some((l0_pa_svc, asid_svc)) = crate::task::process::find_process_l0_user_pa(unsafe { (*t_svc).process_id }) {
                    crate::arch::aarch64::mmu::set_ttbr0_el1(l0_pa_svc, asid_svc);
                }
            }

            // Write return value into the TrapFrame for the asm epilogue.
            unsafe { core::ptr::write_volatile(&mut (*frame).x[0], ret as u64); }
        }
    } else {
        unsafe {
            let far = (*frame).far;
            let elr = (*frame).elr;
            let spsr = (*frame).spsr;
            if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                let far = unsafe { (*frame).far };
                let pid = (*t).process_id;
                crate::log_error!(
                    "EL0-FAULT",
                    "EC={:#x} ESR={:#x} ELR={:#x} FAR={:#x} SPSR={:#x} thread=#{} pid={} -- KILLED thread to prevent looping exception",
                    ec, esr, elr, far, spsr, (*t).id, pid
                );
                crate::task::init_respawn::respawn_init_if_anchor(pid);
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
            let pid = (*t).process_id;
            crate::log_error!(
                "SError-FAULT",
                "EC={:#x} ESR={:#x} ISS={:#x} AET={:#x} ELR={:#x} FAR={:#x} SPSR={:#x} thread=#{} -- KILLED thread to prevent looping exception",
                ec, esr, iss, aet, elr, far, spsr, (*t).id
            );
            // Same init-anchor respawn contract as the sync EL0
            // fault path: if the SError killed the boot anchor, the
            // kernel attempts one respawn of `system/bin/init`
            // before falling through to the scheduler.  See
            // `task::init_respawn` for the rationale on the
            // "consume once" budget.
            crate::task::init_respawn::respawn_init_if_anchor(pid);
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

/// Save DAIF, then mask IRQs (set PSTATE.I).
/// Returns the saved DAIF value for `local_irq_restore`.
#[inline]
pub fn local_irq_save() -> u64 {
    let mut daif: u64;
    unsafe {
        core::arch::asm!(
            "mrs {0}, daif",
            "msr daifset, #2",
            "isb",
            out(reg) daif,
            options(nomem, preserves_flags),
        );
    }
    daif
}

/// Restore DAIF to a previously saved value (from `local_irq_save`).
#[inline]
pub fn local_irq_restore(saved: u64) {
    unsafe {
        core::arch::asm!("msr daif, {0}", in(reg) saved, options(nomem, preserves_flags));
    }
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
