// Boot code for CapsuleOS v0.1.0
.section .text.boot, "ax"
.global _start
.type _start, @function
_start:
    mov x4, x0

    // UART init (PL011 at 0x9000000)
    movz x0, #0x0000
    movk x0, #0x0900, lsl #16
    str wzr, [x0, #0x30]
    mov w1, #13
    str w1, [x0, #0x24]
    mov w1, #2
    str w1, [x0, #0x28]
    mov w1, #0x70
    str w1, [x0, #0x2c]
    mov w1, #0x301
    str w1, [x0, #0x30]
    str wzr, [x0, #0x44]

    // Print "CapsuleOS v0.1.0\r\n"
    movz w1, #0x43
    strb w1, [x0]
    movz w1, #0x61
    strb w1, [x0]
    movz w1, #0x70
    strb w1, [x0]
    movz w1, #0x73
    strb w1, [x0]
    movz w1, #0x75
    strb w1, [x0]
    movz w1, #0x6c
    strb w1, [x0]
    movz w1, #0x65
    strb w1, [x0]
    movz w1, #0x4f
    strb w1, [x0]
    movz w1, #0x53
    strb w1, [x0]
    movz w1, #0x20
    strb w1, [x0]
    movz w1, #0x76
    strb w1, [x0]
    movz w1, #0x30
    strb w1, [x0]
    movz w1, #0x2e
    strb w1, [x0]
    movz w1, #0x31
    strb w1, [x0]
    movz w1, #0x2e
    strb w1, [x0]
    movz w1, #0x30
    strb w1, [x0]
    movz w1, #0x0d
    strb w1, [x0]
    movz w1, #0x0a
    strb w1, [x0]

    // Set up vector table
    adr x0, vector_table
    msr vbar_el1, x0

    // Stack setup
    movz x1, #0xd4df
    movk x1, #0x400, lsl #16
    mov sp, x1

    // Zero BSS
    movz x2, #0xccdd
    movk x2, #0x400c, lsl #16
    add x2, x2, #8
    movz x3, #0xccde
    movk x3, #0x400c, lsl #16
    add x3, x3, #1
bss_zero_loop:
    cmp x2, x3
    b.hs bss_zero_done
    str xzr, [x2], #8
    b bss_zero_loop
bss_zero_done:

    // Call kernel_main
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
