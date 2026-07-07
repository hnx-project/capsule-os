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

    // 3. Enable FP/SIMD at EL1 (CPACR_EL1.FPEN = 3).
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
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 192 bytes, 16-byte aligned)
    sub     sp, sp, #192

    # 2. Save registers x0..x18
    stp     x0,  x1,  [sp, #0]
    stp     x2,  x3,  [sp, #16]
    stp     x4,  x5,  [sp, #32]
    stp     x6,  x7,  [sp, #48]
    stp     x8,  x9,  [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    str     x18,      [sp, #144]

    # 3. Save x30 (LR) at offset 184
    str     x30,      [sp, #184]

    # 4. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    mrs     x9,  spsr_el1
    str     x9,       [sp, #152]
    mrs     x10, elr_el1
    str     x10,      [sp, #160]
    mrs     x11, esr_el1
    str     x11,      [sp, #168]
    mrs     x12, far_el1
    str     x12,      [sp, #176]

    # 5. Pass TrapFrame pointer (sp) in x0 to Rust handler
    mov     x0, sp
    bl      aarch64_sync_el0_handler

    # 6. Restore CSRs
    ldr     x9,       [sp, #152]
    msr     spsr_el1, x9
    ldr     x10,      [sp, #160]
    msr     elr_el1,  x10

    # 7. Restore registers x0..x18 and x30 (LR)
    ldp     x0,  x1,  [sp, #0]
    ldp     x2,  x3,  [sp, #16]
    ldp     x4,  x5,  [sp, #32]
    ldp     x6,  x7,  [sp, #48]
    ldp     x8,  x9,  [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldr     x18,      [sp, #144]
    ldr     x30,      [sp, #184]

    # 8. Deallocate stack space and return via eret
    add     sp, sp, #192
    eret

irq_el0:
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 192 bytes, 16-byte aligned)
    sub     sp, sp, #192

    # 2. Save caller-saved registers x0..x18
    stp     x0,  x1,  [sp, #0]
    stp     x2,  x3,  [sp, #16]
    stp     x4,  x5,  [sp, #32]
    stp     x6,  x7,  [sp, #48]
    stp     x8,  x9,  [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    str     x18,      [sp, #144]

    # 3. Save x30 (LR) at offset 184
    str     x30,      [sp, #184]

    # 4. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    mrs     x9,  spsr_el1
    str     x9,       [sp, #152]
    mrs     x10, elr_el1
    str     x10,      [sp, #160]
    mrs     x11, esr_el1
    str     x11,      [sp, #168]
    mrs     x12, far_el1
    str     x12,      [sp, #176]

    # 5. Ack the GIC: read GICC_IAR (PA 0x0801000C)
    movz    x0, #0x000C
    movk    x0, #0x0801, lsl #16
    ldr     w0, [x0]             // w0 = interrupt ID
    mov     x1, sp               // x1 = trap frame pointer for Rust

    # 6. Early EOI: write the IAR value to GICC_EOIR (PA 0x08010010) immediately
    movz    x2, #0x0010
    movk    x2, #0x0801, lsl #16
    str     w0, [x2]

    # 7. Call Rust dispatcher.  Pass (iar, frame_ptr) in (x0, x1).
    bl      irq_handler

    # If irq_handler returns, no context switch happened: restore and eret.
    # 8. Restore CSRs
    ldr     x9,       [sp, #152]
    msr     spsr_el1, x9
    ldr     x10,      [sp, #160]
    msr     elr_el1,  x10

    # 9. Restore caller-saved registers x0..x18 and x30 (LR)
    ldp     x0,  x1,  [sp, #0]
    ldp     x2,  x3,  [sp, #16]
    ldp     x4,  x5,  [sp, #32]
    ldp     x6,  x7,  [sp, #48]
    ldp     x8,  x9,  [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldr     x18,      [sp, #144]
    ldr     x30,      [sp, #184]

    # 10. Deallocate stack space and return via eret
    add     sp, sp, #192
    eret

fiq_el0:
    mov x0, #0xb00           // 'f' (EL0 FIQ)
    bl diag_trap_class
    b halt

serror_el0:
    mov x0, #0xc00           // 'e' (EL0 SError)
    bl diag_trap_class
    b halt

// ── The main IRQ-EL1 handler ──────────────────────────────────────
// On entry here we are in EL1 with SP_ELx == the kernel's idle stack.
// We ack the GIC, call the Rust dispatcher, then EOI and return.
// Full handler: ack GIC, call Rust dispatcher, EOI, return.
// All trap saves/restores must preserve AAPCS64 (x19-x28 callee-saved).
irq_el1_spx:
    # 1. Allocate space on the stack for TrapFrame + x30 (Total 192 bytes, 16-byte aligned)
    sub     sp, sp, #192

    # 2. Save caller-saved registers x0..x18
    stp     x0,  x1,  [sp, #0]
    stp     x2,  x3,  [sp, #16]
    stp     x4,  x5,  [sp, #32]
    stp     x6,  x7,  [sp, #48]
    stp     x8,  x9,  [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    str     x18,      [sp, #144]

    # 3. Save x30 (LR) at offset 184
    str     x30,      [sp, #184]

    # 4. Save CSRs (spsr_el1, elr_el1, esr_el1, far_el1)
    mrs     x9,  spsr_el1
    str     x9,       [sp, #152]
    mrs     x10, elr_el1
    str     x10,      [sp, #160]
    mrs     x11, esr_el1
    str     x11,      [sp, #168]
    mrs     x12, far_el1
    str     x12,      [sp, #176]

    # 5. Ack the GIC: read GICC_IAR (PA 0x0801000C)
    movz    x0, #0x000C
    movk    x0, #0x0801, lsl #16
    ldr     w0, [x0]             // w0 = interrupt ID

    # 6. Early EOI: write the IAR value to GICC_EOIR (PA 0x08010010) immediately
    # This ensures that GICv2 clears the active status of this IRQ before we potentially
    # context-switch away inside the dispatcher.
    movz    x1, #0x0010
    movk    x1, #0x0801, lsl #16
    str     w0, [x1]

    # 7. Call Rust dispatcher
    bl      irq_handler

    # 8. Restore CSRs
    ldr     x9,       [sp, #152]
    msr     spsr_el1, x9
    ldr     x10,      [sp, #160]
    msr     elr_el1,  x10

    # 9. Restore caller-saved registers x0..x18 and x30 (LR)
    ldp     x0,  x1,  [sp, #0]
    ldp     x2,  x3,  [sp, #16]
    ldp     x4,  x5,  [sp, #32]
    ldp     x6,  x7,  [sp, #48]
    ldp     x8,  x9,  [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldr     x18,      [sp, #144]
    ldr     x30,      [sp, #184]

    # 10. Deallocate stack space and return via eret
    add     sp, sp, #192
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