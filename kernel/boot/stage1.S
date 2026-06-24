.globl _start
_start:
    msr daifset, #0xf
    movz x0, #0x4020, lsl #16
    movk x0, #0x0000, lsl #32
    mov sp, x0
    movz x0, #0x4008, lsl #16
    movk x0, #0x1000, lsl #32
    br x0
