OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH(aarch64)
ENTRY(_start)

MEMORY {
    RAM (xrw) : ORIGIN = 0x40080000, LENGTH = 256M
}

KERNEL_BASE   = 0x40080000;
STACK_SIZE    = 0x8000;

_kernel_start = KERNEL_BASE;

SECTIONS {
    . = KERNEL_BASE;

    .text.boot : {
        KEEP(*(.text.boot))
    } =0

    .text : {
        *(.text)
        *(.text.*)
    } =0

    .rodata : {
        *(.rodata)
        *(.rodata.*)
    } =0

    .data : {
        *(.data)
        *(.data.*)
    } =0

    _bss_start = .;

    .bss (NOLOAD) : {
        *(.bss)
        *(.bss.*)
        *(COMMON)
    } =0

    _bss_end = .;

    . = ALIGN(16);
    __boot_stack_bottom = .;
    . += 0x8000;
    __boot_stack_top = .;

    _kernel_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.debug_*)
        *(.eh_frame)
        *(.eh_frame_hdr)
    }
}
