pub mod scheduler;
pub mod thread;
pub mod process;
pub mod smoke;

pub use scheduler::Scheduler;
pub use thread::{Thread, Priority};
pub use process::Process;

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    r#"
.section .text
.global switch_to
switch_to:
    // Save callee-saved registers x19-x30 from the *current* thread.
    // The caller passes x0 = current ThreadContext*, x1 = next ThreadContext*.
    stp x19, x20, [x0, #152]   // current.r[0..1] -- offset after x[0..18]
    stp x21, x22, [x0, #168]
    stp x23, x24, [x0, #184]
    stp x25, x26, [x0, #200]
    stp x27, x28, [x0, #216]
    stp x29, x30, [x0, #232]
    // Save the kernel's SP (sp_el1).
    mov x2, sp
    str x2, [x0, #248]         // current.sp
    // Save the user-mode SP (sp_el0).
    mrs x3, sp_el0
    str x3, [x0, #256]         // current.user_sp

    // Load callee-saved registers for the next thread.
    ldp x19, x20, [x1, #152]
    ldp x21, x22, [x1, #168]
    ldp x23, x24, [x1, #184]
    ldp x25, x26, [x1, #200]
    ldp x27, x28, [x1, #216]
    ldp x29, x30, [x1, #232]
    // Load the next thread's kernel SP and user SP (sp_el0).
    ldr x2, [x1, #248]
    mov sp, x2
    ldr x4, [x1, #256]
    msr sp_el0, x4

    // x0..x18 of the next thread will be restored by user_eret_stub from
    // ThreadContext.x; for now just hand off.
    ret
    "#
);

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    r#"
.section .text
.global switch_to
switch_to:
    sd s0, 0(a0)
    sd s1, 8(a0)
    sd s2, 16(a0)
    sd s3, 24(a0)
    sd s4, 32(a0)
    sd s5, 40(a0)
    sd s6, 48(a0)
    sd s7, 56(a0)
    sd s8, 64(a0)
    sd s9, 72(a0)
    sd s10, 80(a0)
    sd s11, 88(a0)
    // Layout matches ThreadContext on RISC-V: sp at 96, ra at 104.  We do
    // not model a separate user sp here because S-Mode swaps sp on trap.
    sd sp, 96(a0)
    sd ra, 104(a0)

    ld s0, 0(a1)
    ld s1, 8(a1)
    ld s2, 16(a1)
    ld s3, 24(a1)
    ld s4, 32(a1)
    ld s5, 40(a1)
    ld s6, 48(a1)
    ld s7, 56(a1)
    ld s8, 64(a1)
    ld s9, 72(a1)
    ld s10, 80(a1)
    ld s11, 88(a1)
    ld sp, 96(a1)
    ld ra, 104(a1)

    ret
    "#
);

extern "C" {
    pub fn switch_to(current: *mut thread::ThreadContext, next: *const thread::ThreadContext);
}

