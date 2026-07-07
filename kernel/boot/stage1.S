.section .text, "ax"
.global _start
.type _start, @function
_start:
    // Disable interrupts
    msr daifset, #0xf

    // Set up initial stack
    movz x0, #0x0000
    movk x0, #0x4020, lsl #16
    mov sp, x0

    // Copy kernel from 0x40080000 to 0x40000000
    // Kernel size is approximately 0x40000 bytes (256KB)
    mov x1, #0x40000000      // dest
    mov x2, #0x40080000      // src
    mov x3, #0x40000         // size (256KB)

copy_loop:
    cmp x3, #0
    beq copy_done
    sub x3, x3, #1
    ldr x4, [x2], #8
    str x4, [x1], #8
    b copy_loop

copy_done:
    // Jump to kernel at 0x40000000
    movz x0, #0x0000
    movk x0, #0x4000, lsl #16
    br x0
