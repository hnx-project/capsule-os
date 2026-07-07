//! RISC-V 64 exception and interrupt (trap) handling.

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    pub regs: [u64; 32],
    pub sstatus: u64,
    pub sepc: u64,
    pub scause: u64,
    pub stval: u64,
}

#[no_mangle]
pub extern "C" fn riscv64_trap_handler(scause: usize, sepc: usize, stval: usize, frame: *mut TrapFrame) {
    let is_interrupt = (scause as isize) < 0;
    let code = scause & 0xfff;

    if is_interrupt {
        match code {
            5 => {
                // Supervisor Timer Interrupt (STIP)
                crate::drivers::timer::handle_tick();
            }
            _ => {
                panic_unhandled(frame, scause, sepc, stval);
            }
        }
    } else {
        match code {
            8 => {
                // Environment call from U-mode (User syscall)
                unsafe {
                    let syscall_num = (*frame).regs[17]; // a7
                    let arg0 = (*frame).regs[10]; // a0
                    let arg1 = (*frame).regs[11]; // a1
                    let arg2 = (*frame).regs[12]; // a2
                    let arg3 = (*frame).regs[13]; // a3
                    let arg4 = (*frame).regs[14]; // a4
                    let arg5 = (*frame).regs[15]; // a5

                    let ret = crate::syscall::syscall_dispatch(
                        syscall_num as u32,
                        arg0 as usize,
                        arg1 as usize,
                        arg2 as usize,
                        arg3 as usize,
                        arg4 as usize,
                        arg5 as usize,
                    );

                    (*frame).regs[10] = ret as u64; // Return value in a0
                    (*frame).sepc += 4;             // Skip ecall
                }
            }
            _ => {
                panic_unhandled(frame, scause, sepc, stval);
            }
        }
    }
}

#[cold]
#[inline(never)]
fn panic_unhandled(_frame: *mut TrapFrame, scause: usize, sepc: usize, stval: usize) {
    panic!(
        "Unhandled exception: scause={:#x}, sepc={:#x}, stval={:#x}",
        scause, sepc, stval
    );
}

/// Disable interrupts globally at S-Mode level (clear SIE in sstatus).
#[inline]
pub fn disable_irqs() {
    unsafe {
        core::arch::asm!("csrc sstatus, {0}", in(reg) 1 << 1, options(nomem, preserves_flags));
    }
}

/// Enable interrupts globally at S-Mode level (set SIE in sstatus).
#[inline]
pub fn enable_irqs() {
    unsafe {
        core::arch::asm!("csrs sstatus, {0}", in(reg) 1 << 1, options(nomem, preserves_flags));
    }
}

/// Enable timer interrupt in sie (Supervisor Timer Interrupt Enable - STIE).
#[inline]
pub fn enable_timer_interrupt() {
    unsafe {
        core::arch::asm!("csrs sie, {0}", in(reg) 1 << 5, options(nomem, preserves_flags));
    }
}
