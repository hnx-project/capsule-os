ENTRY(_start)

SECTIONS {
    . = 0x80200000; /* QEMU virt RISC-V OpenSBI 默认引导加载地址 */
    
    .text : {
        *(.text.entry) /* 确保入口汇编在最前 */
        *(.text .text.*)
    }
    
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    
    .bss : {
        _bss_start = .;
        *(.bss .bss.*)
        _bss_end = .;
    }

    /* 动态分配 64KB 作为启动栈，解决硬编码地址冲突 */
    . = ALIGN(16);
    _stack_bottom = .;
    . += 0x10000;
    _stack_top = .;
}
