// Boot code for CapsuleOS v0.2.0
.section .text.boot, "ax"
.global _start
.type _start, @function
_start:
    // 1. Save DTB pointer (passed in x0) in x19 (callee-saved register)
    //    必须最先做，后续任何使用 x0 的指令都会覆盖它
    mov x19, x0

    // 2. 启用 FP/SIMD at EL1 (CPACR_EL1.FPEN = 3)
    mov x1, #0x300000
    msr cpacr_el1, x1
    isb

    // 3. Set up vector table
    adr x0, vector_table
    msr vbar_el1, x0

    // 4. Stack setup using linker script symbol
    ldr x1, =__boot_stack_top
    mov sp, x1

    // 5. Zero BSS section dynamically using linker script symbols
    ldr x1, =_bss_start
    ldr x2, =_bss_end
bss_zero_loop:
    cmp x1, x2
    b.hs bss_zero_done
    str xzr, [x1], #8
    b bss_zero_loop
bss_zero_done:

    // 6. Restore DTB pointer to x0 as the first parameter for kernel_main
    mov x0, x19
    bl kernel_main

halt:
    b halt

// Simple exception vector table
.align 11
vector_table:
    b sync_exception
    b irq_exception
    b fiq_exception
    b serror_exception
    b sync_exception
    b irq_exception
    b fiq_exception
    b serror_exception
    b sync_exception
    b irq_exception
    b fiq_exception
    b serror_exception
    b sync_exception
    b irq_exception
    b fiq_exception
    b serror_exception

sync_exception:
    b sync_exception

irq_exception:
    movz x0, #0x0000
    movk x0, #0x0900, lsl #16
    movz w1, #0x49
    strb w1, [x0]
    eret

fiq_exception:
    movz x0, #0x0000
    movk x0, #0x0900, lsl #16
    movz w1, #0x46
    strb w1, [x0]
    eret

serror_exception:
    movz x0, #0x0000
    movk x0, #0x0900, lsl #16
    movz w1, #0x45
    strb w1, [x0]
    eret
