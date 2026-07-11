// Boot code for CapsuleOS v3.0 (Phase 3.x interrupt plumbing).
.section .text.boot, "ax"
.global _start
.type _start, @function
_start:
    // 0. EL2 to EL1 drop.  QEMU PSCI firmware sets HCR_EL2 IMO+FMO bits
    //    when booting at EL1, routing IRQs/FIQs to EL2.  If we start at
    //    EL2 (e.g. psci=off) we clear those bits before entering EL1.
    mrs     x15, CurrentEL
    cmp     x15, #(2 << 2)          // EL2 encoded as 0b10 in bits [3:2]
    b.ne    0f
    msr     sctlr_el2, xzr          // Disable EL2 MMU/if any
    msr     cptr_el2, xzr           // Disable all EL2 traps
    isb
    mov     x15, #(1 << 31)         // HCR_EL2.RW = AArch64 at EL1; IMO=FMO=TGE=0
    msr     hcr_el2, x15
    isb
    mov     x15, #0x3C5             // SPSR: EL1h (SP_EL1), all DAIF masked
    adr     x16, 0f                 // Return to _start_el1
    msr     spsr_el2, x15
    msr     elr_el2, x16
    isb
    eret
0:
    // _start_el1: we are now at EL1
    // 1. Save DTB pointer (passed in x0) in x19 (callee-saved).
    mov     x19, x0

    // 2. Use SP_EL1 for the kernel's stack.  The kernel always runs
    //    with SP_ELx (so the trap stubs' IRQ-EL1-SP_ELx slot fires).
    msr     spsel, #1

    // 3. Enable FP/SIMD at EL1 & EL0 (CPACR_EL1.FPEN = 3).
    // Bits [21:20] = 0b11 (Non-trapping for EL0 and EL1)
    mov     x1, #0x300000
    msr     cpacr_el1, x1
    isb

    // 4. Install vector table at vbar_el1.  Must be 2 KiB aligned.
    //    `vector_table` lives in this same .text section right below.
    adr     x0, vector_table
    msr     vbar_el1, x0
    isb

    // 4. Set up the kernel's idle stack from the linker symbol.
    ldr x1, =__boot_stack_top
    mov sp, x1

    // 5. Zero BSS dynamically using the linker symbols.
    ldr x1, =_bss_start
    ldr x2, =_bss_end
bss_zero_loop:
    cmp x1, x2
    b.hs bss_zero_done
    str xzr, [x1], #8
    b bss_zero_loop
bss_zero_done:

    // 7. Hand off to Rust with DTB pointer in x0.
    mov x0, x19
    bl kernel_main

halt:
    b halt

// ───────────────────────────────────────────────────────────────────
// Exception vector table.  Each entry is exactly 128 bytes (AArch64
//  VBAR alignment requirement); entries must be 128 bytes apart so the
//  CPU lands at the correct offset for each exception type.
// ───────────────────────────────────────────────────────────────────
.align 11
vector_table:
    // 0x000: Synchronous, EL1 with SP_EL0
    b sync_el1_sp0
    .space 128-4, 0
    // 0x080: IRQ,       EL1 with SP_EL0
    b irq_el1_sp0
    .space 128-4, 0
    // 0x100: FIQ,       EL1 with SP_EL0
    b fiq_el1_sp0
    .space 128-4, 0
    // 0x180: SError,    EL1 with SP_EL0
    b serror_el1_sp0
    .space 128-4, 0

    // 0x200: Synchronous, EL1 with SP_ELx
    b sync_el1_spx
    .space 128-4, 0
    // 0x280: IRQ,       EL1 with SP_ELx   <-- IRQ fires here
    b irq_el1_spx
    .space 128-4, 0
    // 0x300: FIQ,       EL1 with SP_ELx
    b fiq_el1_spx
    .space 128-4, 0
    // 0x380: SError,    EL1 with SP_ELx
    b serror_el1_spx
    .space 128-4, 0

    // 0x400: Synchronous, EL0 (64-bit)
    b sync_el0
    .space 128-4, 0
    // 0x480: IRQ,       EL0 (64-bit)
    b irq_el0
    .space 128-4, 0
    // 0x500: FIQ,       EL0 (64-bit)
    b fiq_el0
    .space 128-4, 0
    // 0x580: SError,    EL0 (64-bit)
    b serror_el0
    .space 128-4, 0

    // 0x600: Synchronous, EL0 (32-bit)  -- not used, branch to sync_el0
    b sync_el0
    .space 128-4, 0
    // 0x680: IRQ,       EL0 (32-bit)  -- not used, branch to irq_el0
    b irq_el0
    .space 128-4, 0
    // 0x700: FIQ,       EL0 (32-bit)
    b fiq_el0
    .space 128-4, 0
    // 0x780: SError,    EL0 (32-bit)
    b serror_el0
    .space 128-4, 0

// Trap stubs.  Each one:
//   * switches to the trap stack,
//   * builds a `TrapFrame` containing x0..x18, spsr, elr, esr, far,
//   * calls the Rust dispatcher,
//   * restores x0..x18 from the frame and `eret`s.
//
// We only fully support the IRQ-EL1-SP_ELx path right now; the
// other branches go to a `halt` loop after a diagnostic print.

sync_el1_sp0:
    mov x0, #0x100           // 'S'
    bl diag_trap_class
    b halt

irq_el1_sp0:
    mov x0, #0x200           // 'I'
    bl diag_trap_class
    b halt

fiq_el1_sp0:
    mov x0, #0x300           // 'F'
    bl diag_trap_class
    b halt

serror_el1_sp0:
    mov x0, #0x400           // 'E'
    bl diag_trap_class
    b halt

sync_el1_spx:
    mov x0, #0x500           // 'S'
    bl diag_trap_class
    b halt

fiq_el1_spx:
    // Minimal test: stamp 'F' to PL011 and return.
    movz x1, #0x0000
    movk x1, #0x0900, lsl #16
    mov w0, #'F'
    strb w0, [x1]
    eret

serror_el1_spx:
    mov x0, #0x700           // 'E'
    bl diag_trap_class
    b halt

sync_el0:
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 208 bytes: 19 x regs (152) + spsr/elr/esr/far/lr/sp_el0 (48) + 8 padding for 16-byte alignment, see kernel TrapFrame struct)
    sub     sp, sp, #208

    # 2. Stash the entry sp in x19 (callee-saved per AAPCS64, so the
    #    Rust handler below must preserve it) so the restore phase can
    #    still find the TrapFrame even if a nested IRQ handler ends up
    #    mutating sp via switch_to + eret.  Without this, an IRQ nested
    #    into a blocking syscall (e.g. SYSCALL_READ waiting on the UART)
    #    can leave the tail-end `ldp x0, x1, [sp, #0]` reading from
    #    stack memory belonging to the IRQ handler's Rust frame, so the
    #    EL0 user thread observes whatever x0 the IRQ handler happened
    #    to leave in memory (commonly the user's stack pointer from
    #    before the svc) instead of the syscall's true return value.
    mov     x19, sp

    # 3. Save registers x0..x18
    stp     x0,  x1,  [x19, #0]
    stp     x2,  x3,  [x19, #16]
    stp     x4,  x5,  [x19, #32]
    stp     x6,  x7,  [x19, #48]
    stp     x8,  x9,  [x19, #64]
    stp     x10, x11, [x19, #80]
    stp     x12, x13, [x19, #96]
    stp     x14, x15, [x19, #112]
    stp     x16, x17, [x19, #128]
    str     x18,      [x19, #144]

    # 4. Save x30 (LR) at offset 184
    str     x30,      [x19, #184]

    # 5. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    mrs     x9,  spsr_el1
    str     x9,       [x19, #152]
    mrs     x10, elr_el1
    str     x10,      [x19, #160]
    mrs     x11, esr_el1
    str     x11,      [x19, #168]
    mrs     x12, far_el1
    str     x12,      [x19, #176]

    # 5b. Save the user-mode SP (sp_el0) at offset 192 so the SVC
    #     epilogue can restore the *user* sp, not the kernel sp we
    #     just `sub`d.  Without this the user-mode program resumes
    #     with a kernel stack pointer in sp_el0 and the first stack
    #     access after the SVC faults as a Data Abort at FAR=sp_el0.
    mrs     x13, sp_el0
    str     x13,      [x19, #192]

    # 6. Pass TrapFrame pointer in x0 to Rust handler
    mov     x0, x19
    bl      aarch64_sync_el0_handler

    # 7. Restore CSRs from the original TrapFrame location.
    ldr     x9,       [x19, #152]
    msr     spsr_el1, x9
    ldr     x10,      [x19, #160]
    msr     elr_el1,  x10
    isb
    # **Critical**: write ELR_EL1 a second time after the barrier.
    # QEMU's TCG (cortex-a72 emulation on x86) appears to require the
    # ELR_EL1 write to be issued immediately before the eret, not
    # interleaved with the register restore.  Without this second
    # write, the eret uses a stale ELR_EL1 left over from a nested
    # IRQ handler that ran during the syscall dispatcher — typically
    # the IRQ's "mrs elr_el1; str; ...; mrs elr_el1 = handler PC;
    # msr spsr; msr elr; isb" — and the user-mode program jumps into
    # a kernel VA, producing EC=0x20 (instruction abort) on the next
    # instruction fetch.
    msr     elr_el1,  x10
    isb

    # 7b. Restore the user-mode SP (sp_el0) so the eret returns to
    #     EL0 with the original user stack pointer — not the kernel
    #     sp we `add`d back.  Loaded from the sp slot (offset 192).
    ldr     x11,      [x19, #192]
    msr     sp_el0,   x11
    isb

    # 8. Restore registers x0..x18 and x30 (LR) from the TrapFrame.
    ldp     x0,  x1,  [x19, #0]
    ldp     x2,  x3,  [x19, #16]
    ldp     x4,  x5,  [x19, #32]
    ldp     x6,  x7,  [x19, #48]
    ldp     x8,  x9,  [x19, #64]
    ldp     x10, x11, [x19, #80]
    ldp     x12, x13, [x19, #96]
    ldp     x14, x15, [x19, #112]
    ldp     x16, x17, [x19, #128]
    ldr     x18,      [x19, #144]
    ldr     x30,      [x19, #184]

    # 9. Deallocate stack space and return via eret
    add     sp, sp, #208
    eret

irq_el0:
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 208 bytes: 19 x regs (152) + spsr/elr/esr/far/lr/sp_el0 (48) + 8 padding for 16-byte alignment, see kernel TrapFrame struct)
    sub     sp, sp, #208

    # 2. Stash entry sp in x19 (callee-saved) so we can still find the
    #    TrapFrame after a nested IRQ + scheduler switch_to.
    mov     x19, sp

    # 3. Save caller-saved registers x0..x18
    stp     x0,  x1,  [x19, #0]
    stp     x2,  x3,  [x19, #16]
    stp     x4,  x5,  [x19, #32]
    stp     x6,  x7,  [x19, #48]
    stp     x8,  x9,  [x19, #64]
    stp     x10, x11, [x19, #80]
    stp     x12, x13, [x19, #96]
    stp     x14, x15, [x19, #112]
    stp     x16, x17, [x19, #128]
    str     x18,      [x19, #144]

    # 4. Save x30 (LR) at offset 184
    str     x30,      [x19, #184]

    # 5. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    mrs     x9,  spsr_el1
    str     x9,       [x19, #152]
    mrs     x10, elr_el1
    str     x10,      [x19, #160]
    mrs     x11, esr_el1
    str     x11,      [x19, #168]
    mrs     x12, far_el1
    str     x12,      [x19, #176]

    # 5b. Save the user-mode SP (sp_el0) at offset 192 so the SVC
    #     epilogue can restore the *user* sp, not the kernel sp we
    #     just `sub`d.  Without this the user-mode program resumes
    #     with a kernel stack pointer in sp_el0 and the first stack
    #     access after the SVC faults as a Data Abort at FAR=sp_el0.
    mrs     x13, sp_el0
    str     x13,      [x19, #192]

    # 6. Ack the GIC: read GICC_IAR (PA 0x0801000C)
    movz    x0, #0x000C
    movk    x0, #0x0801, lsl #16
    ldr     w0, [x0]             // w0 = interrupt ID
    mov     x1, x19              // x1 = trap frame pointer for Rust

    # 7. Early EOI: write the IAR value to GICC_EOIR (PA 0x08010010) immediately
    movz    x2, #0x0010
    movk    x2, #0x0801, lsl #16
    str     w0, [x2]

    # 8. Call Rust dispatcher.  Pass (iar, frame_ptr) in (x0, x1).
    bl      irq_handler

    # If irq_handler returns, no context switch happened: restore and eret.
    # 9. Restore CSRs from the TrapFrame captured at entry.
    ldr     x9,       [x19, #152]
    msr     spsr_el1, x9
    ldr     x10,      [x19, #160]
    msr     elr_el1,  x10
    isb
    msr     elr_el1,  x10
    isb

    # 9b. Restore the user-mode SP (sp_el0) so the eret returns to
    #     EL0 with the original user stack pointer.  Loaded from
    #     the sp slot at offset 192.
    ldr     x11,      [x19, #192]
    msr     sp_el0,   x11

    # 10. Restore caller-saved registers x0..x18 and x30 (LR) from TrapFrame.
    ldp     x0,  x1,  [x19, #0]
    ldp     x2,  x3,  [x19, #16]
    ldp     x4,  x5,  [x19, #32]
    ldp     x6,  x7,  [x19, #48]
    ldp     x8,  x9,  [x19, #64]
    ldp     x10, x11, [x19, #80]
    ldp     x12, x13, [x19, #96]
    ldp     x14, x15, [x19, #112]
    ldp     x16, x17, [x19, #128]
    ldr     x18,      [x19, #144]
    ldr     x30,      [x19, #184]

    # 11. Deallocate stack space and return via eret
    add     sp, sp, #208
    eret

fiq_el0:
    mov x0, #0xb00           // 'f' (EL0 FIQ)
    bl diag_trap_class
    b halt

serror_el0:
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 208 bytes: 19 x regs (152) + spsr/elr/esr/far/lr/sp_el0 (48) + 8 padding for 16-byte alignment, see kernel TrapFrame struct)
    sub     sp, sp, #208

    # 2. Stash the entry sp in x19 (callee-saved per AAPCS64) so the
    #    restore phase can still find the TrapFrame even if the Rust
    #    dispatcher triggers a nested IRQ + scheduler switch_to.
    mov     x19, sp

    # 3. Save registers x0..x18
    stp     x0,  x1,  [x19, #0]
    stp     x2,  x3,  [x19, #16]
    stp     x4,  x5,  [x19, #32]
    stp     x6,  x7,  [x19, #48]
    stp     x8,  x9,  [x19, #64]
    stp     x10, x11, [x19, #80]
    stp     x12, x13, [x19, #96]
    stp     x14, x15, [x19, #112]
    stp     x16, x17, [x19, #128]
    str     x18,      [x19, #144]

    # 4. Save x30 (LR) at offset 184
    str     x30,      [x19, #184]

    # 5. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    #    **For SError we keep ELR_EL1 as captured** — the exception is
    #    asynchronous, so ELR does not point at the faulting instruction
    #    and `elr += 4` (which the SVC path does) would be wrong.
    mrs     x9,  spsr_el1
    str     x9,       [x19, #152]
    mrs     x10, elr_el1
    str     x10,      [x19, #160]
    mrs     x11, esr_el1
    str     x11,      [x19, #168]
    mrs     x12, far_el1
    str     x12,      [x19, #176]

    # 5b. Save the user-mode SP (sp_el0) at offset 192 so the epilogue
    #     restores the *user* sp, not the kernel sp we just `sub`d.
    mrs     x13, sp_el0
    str     x13,      [x19, #192]

    # 6. Pass TrapFrame pointer in x0 to the Rust dispatcher.
    mov     x0, x19
    bl      aarch64_serror_el0_handler

    # 7. Restore CSRs from the TrapFrame.
    ldr     x9,       [x19, #152]
    msr     spsr_el1, x9
    ldr     x10,      [x19, #160]
    msr     elr_el1,  x10
    isb
    # **Critical**: write ELR_EL1 a second time after the barrier
    # (same QEMU-TCG workaround as `sync_el0`).
    msr     elr_el1,  x10
    isb

    # 7b. Restore the user-mode SP (sp_el0) so the eret returns to EL0
    #     with the original user stack pointer.
    ldr     x11,      [x19, #192]
    msr     sp_el0,   x11
    isb

    # 8. Restore registers x0..x18 and x30 (LR) from the TrapFrame.
    ldp     x0,  x1,  [x19, #0]
    ldp     x2,  x3,  [x19, #16]
    ldp     x4,  x5,  [x19, #32]
    ldp     x6,  x7,  [x19, #48]
    ldp     x8,  x9,  [x19, #64]
    ldp     x10, x11, [x19, #80]
    ldp     x12, x13, [x19, #96]
    ldp     x14, x15, [x19, #112]
    ldp     x16, x17, [x19, #128]
    ldr     x18,      [x19, #144]
    ldr     x30,      [x19, #184]

    # 9. Deallocate stack space and return via eret
    add     sp, sp, #208
    eret

// ── The main IRQ-EL1 handler ──────────────────────────────────────
// On entry here we are in EL1 with SP_ELx == the kernel's idle stack.
// We ack the GIC, call the Rust dispatcher, then EOI and return.
// Full handler: ack GIC, call Rust dispatcher, EOI, return.
// All trap saves/restores must preserve AAPCS64 (x19-x28 callee-saved).
irq_el1_spx:
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 208 bytes: 19 x regs (152) + spsr/elr/esr/far/lr/sp_el0 (48) + 8 padding for 16-byte alignment, see kernel TrapFrame struct)
    sub     sp, sp, #208

    # 2. Stash entry sp in x19 (callee-saved) so we can still find the
    #    TrapFrame after a nested IRQ + scheduler switch_to.
    mov     x19, sp

    # 3. Save caller-saved registers x0..x18
    stp     x0,  x1,  [x19, #0]
    stp     x2,  x3,  [x19, #16]
    stp     x4,  x5,  [x19, #32]
    stp     x6,  x7,  [x19, #48]
    stp     x8,  x9,  [x19, #64]
    stp     x10, x11, [x19, #80]
    stp     x12, x13, [x19, #96]
    stp     x14, x15, [x19, #112]
    stp     x16, x17, [x19, #128]
    str     x18,      [x19, #144]

    # 4. Save x30 (LR) at offset 184
    str     x30,      [x19, #184]

    # 5. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    mrs     x9,  spsr_el1
    str     x9,       [x19, #152]
    mrs     x10, elr_el1
    str     x10,      [x19, #160]
    mrs     x11, esr_el1
    str     x11,      [x19, #168]
    mrs     x12, far_el1
    str     x12,      [x19, #176]

    # 5b. Save the user-mode SP (sp_el0) at offset 192 so the SVC
    #     epilogue can restore the *user* sp, not the kernel sp we
    #     just `sub`d.  Without this the user-mode program resumes
    #     with a kernel stack pointer in sp_el0 and the first stack
    #     access after the SVC faults as a Data Abort at FAR=sp_el0.
    mrs     x13, sp_el0
    str     x13,      [x19, #192]

    # 6. Ack the GIC: read GICC_IAR (PA 0x0801000C)
    movz    x0, #0x000C
    movk    x0, #0x0801, lsl #16
    ldr     w0, [x0]             // w0 = interrupt ID

    # 7. Early EOI: write the IAR value to GICC_EOIR (PA 0x08010010) immediately
    # This ensures that GICv2 clears the active status of this IRQ before we potentially
    # context-switch away inside the dispatcher.
    movz    x1, #0x0010
    movk    x1, #0x0801, lsl #16
    str     w0, [x1]

    # 8. Call Rust dispatcher.  Pass (iar, frame_ptr) in (x0, x1).
    mov     x1, x19
    bl      irq_handler

    # 9. Restore CSRs from the TrapFrame captured at entry.
    ldr     x9,       [x19, #152]
    msr     spsr_el1, x9
    ldr     x10,      [x19, #160]
    msr     elr_el1,  x10
    isb
    msr     elr_el1,  x10
    isb

    # 9b. Restore the user-mode SP (sp_el0) so the eret returns to
    #     EL0 with the original user stack pointer.  Loaded from
    #     the sp slot at offset 192.
    ldr     x11,      [x19, #192]
    msr     sp_el0,   x11

    # 10. Restore caller-saved registers x0..x18 and x30 (LR) from TrapFrame.
    ldp     x0,  x1,  [x19, #0]
    ldp     x2,  x3,  [x19, #16]
    ldp     x4,  x5,  [x19, #32]
    ldp     x6,  x7,  [x19, #48]
    ldp     x8,  x9,  [x19, #64]
    ldp     x10, x11, [x19, #80]
    ldp     x12, x13, [x19, #96]
    ldp     x14, x15, [x19, #112]
    ldp     x16, x17, [x19, #128]
    ldr     x18,      [x19, #144]
    ldr     x30,      [x19, #184]

    # 11. Deallocate stack space and return via eret
    add     sp, sp, #208
    eret

// ── Diagnostic for traps we don't expect yet ──────────────────────
// Marked .global so the linker keeps it; called from the placeholder
// stubs above.
.global diag_trap_class
diag_trap_class:
    // a0 holds the tag (low byte is the ascii class letter).
    // Print "<tag>!" to UART 0x0900_0000 and return.
    movz x1, #0x0000
    movk x1, #0x0900, lsl #16
    uxtb w0, w0
    strb w0, [x1]
    mov w0, #'!'
    strb w0, [x1]
    ret

// ── BSS linker symbol that the linker script should provide ──────
.global __trap_stack_top
__trap_stack_top:
    .skip 16384