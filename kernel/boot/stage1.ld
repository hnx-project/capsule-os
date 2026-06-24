OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH(aarch64)
ENTRY(_start)

SECTIONS {
    . = 0x40000000;

    .text : {
        *(.text)
    } =0

    /DISCARD/ : {
        *(.comment)
        *(.debug_*)
        *(.eh_frame)
    }
}
