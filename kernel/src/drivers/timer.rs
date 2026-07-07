//! Cross-platform generic timer driver.
//!
//! Handles AArch64 EL1 physical counter timer and RISC-V 64 S-Mode timer via OpenSBI.

#[cfg(target_arch = "aarch64")]
mod arch_timer {
    pub const TIMER_INTERVAL_TICKS: u64 = 1_000_000; // ~16 ms at 62.5 MHz QEMU default
    pub static mut TIMER_FREQ_HZ: u64 = 0;

    #[inline(always)]
    fn read_cntfrq() -> u64 {
        unsafe {
            let v: u64;
            core::arch::asm!("mrs {0}, cntfrq_el0", out(reg) v, options(nomem, preserves_flags));
            v
        }
    }

    #[inline(always)]
    pub fn read_cntpct() -> u64 {
        unsafe {
            let v: u64;
            core::arch::asm!("mrs {0}, cntpct_el0", out(reg) v, options(nomem, preserves_flags));
            v
        }
    }

    #[inline(always)]
    fn write_cntp_tval(val: u64) {
        unsafe {
            core::arch::asm!("msr cntp_tval_el0, {0}", in(reg) val, options(nomem, preserves_flags));
        }
    }

    #[inline(always)]
    fn write_cntp_ctl(val: u64) {
        unsafe {
            core::arch::asm!("msr cntp_ctl_el0, {0}", in(reg) val, options(nomem, preserves_flags));
        }
    }

    pub fn init() {
        unsafe {
            TIMER_FREQ_HZ = read_cntfrq();
            write_cntp_tval(TIMER_INTERVAL_TICKS);
            write_cntp_ctl(0b001); // enable=1, imask=0
        }
    }

    pub fn rearm() {
        write_cntp_tval(TIMER_INTERVAL_TICKS);
    }
}

#[cfg(target_arch = "riscv64")]
mod arch_timer {
    pub const TIMER_INTERVAL_TICKS: u64 = 100_000; // 10 ms at 10 MHz default
    pub const TIMER_FREQ_HZ: u64 = 10_000_000;

    #[inline(always)]
    pub fn read_time() -> u64 {
        unsafe {
            let v: u64;
            core::arch::asm!("csrr {0}, time", out(reg) v, options(nomem, preserves_flags));
            v
        }
    }

    #[inline(always)]
    fn sbi_set_timer(time_value: u64) {
        unsafe {
            // Write directly to the hardware stimecmp CSR (0x14d) under the Sstc extension
            core::arch::asm!(
                "csrw 0x14d, {0}",
                in(reg) time_value,
                options(nomem, preserves_flags)
            );
        }
    }

    pub fn init() {
        let t = read_time();
        let next = t + TIMER_INTERVAL_TICKS;
        sbi_set_timer(next);

        // Enable timer interrupt in S-Mode now
        crate::arch::riscv64::trap::enable_timer_interrupt();
    }

    pub fn rearm() {
        let next = read_time() + TIMER_INTERVAL_TICKS;
        sbi_set_timer(next);
    }
}

static mut TICK_COUNT: u64 = 0;

/// Initialize the architecture-specific timer.
pub fn init() {
    arch_timer::init();
}

/// Read the currently-programmed tick interval (in counter ticks).
pub fn interval_ticks() -> u64 {
    arch_timer::TIMER_INTERVAL_TICKS
}

/// Read the timer frequency (Hz).
pub fn freq_hz() -> u64 {
    #[cfg(target_arch = "aarch64")]
    unsafe { arch_timer::TIMER_FREQ_HZ }
    #[cfg(target_arch = "riscv64")]
    arch_timer::TIMER_FREQ_HZ
}

/// Read the 64-bit physical counter.
pub fn phys_count() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        arch_timer::read_cntpct()
    }
    #[cfg(target_arch = "riscv64")]
    {
        arch_timer::read_time()
    }
}

pub fn get_ticks() -> u64 {
    unsafe { TICK_COUNT }
}

/// Called from the IRQ dispatcher (IRQ-EL0 / IRQ-EL1 paths) on every timer
/// tick.  `frame` is the kernel trap-frame pointer; on AArch64 EL0 IRQ it
/// is non-null and lets us persist x0..x18 into the current thread's
/// ThreadContext before the scheduler potentially context-switches away.
#[cfg(target_arch = "aarch64")]
pub fn handle_tick_from_irq(frame: *mut crate::arch::aarch64::trap::TrapFrame) {
    unsafe {
        TICK_COUNT += 1;
        let count = TICK_COUNT;

        // Re-arm timer first
        arch_timer::rearm();

        // Stash the interrupted user's PC + x0..x18 + spsr into the current
        // thread's ThreadContext.  This makes the next switch_to on this
        // thread resume exactly here instead of restarting from _start or
        // clobbering caller-saved registers.
        let elr: u64;
        let spsr: u64;
        core::arch::asm!(
            "mrs {0}, elr_el1",
            "mrs {1}, spsr_el1",
            out(reg) elr,
            out(reg) spsr,
            options(nomem, nostack)
        );
        if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
            unsafe {
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

        // Lightweight print, throttled to every 100 ticks.
        if count % 100 == 0 {
            crate::log_info!("TIMER", "tick {}", count);
        }
    }
}

/// Backwards-compatible alias for IRQ paths that don't pass a frame
/// (e.g. RISC-V or kernel-mode IRQs that never preempt into a user thread).
#[cfg(not(target_arch = "aarch64"))]
pub fn handle_tick_from_irq(_frame: *mut ()) {
    handle_tick();
}

pub fn handle_tick() {
    unsafe {
        TICK_COUNT += 1;
        let count = TICK_COUNT;

        // Re-arm timer first
        arch_timer::rearm();

        // Trigger the preemptive scheduler (no frame persistence here).
        crate::task::scheduler::SCHEDULER.schedule();

        // Lightweight print, throttled to every 100 ticks.
        if count % 100 == 0 {
            crate::log_info!("TIMER", "tick {}", count);
        }
    }
}
