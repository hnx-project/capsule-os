MEMORY {
    RAM (xrw) : ORIGIN = 0x40080000, LENGTH = 256M
}

ENTRY(_start)

SECTIONS {
    .text 0x40080000 : {
        *(.text.boot)
        *(.text._start)
        *(.text)
        *(.rodata)
    } > RAM

    .data : {
        *(.data)
    } > RAM

    .bss (NOLOAD) : {
        *(.bss)
        *(COMMON)
        . = ALIGN(8);
        _end = .;
    } > RAM

    _stack_start = 0x40200000;
    _stack_size = 0x10000;
}
