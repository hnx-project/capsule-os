.section .text.boot, "ax"
.global _start
.type _start, @function
_start:
    // 1. 设置栈指针：利用链接脚本导出的通用符号 __boot_stack_top
    la sp, __boot_stack_top

    // 2. 动态 BSS 清零：利用链接脚本符号 _bss_start 和 _bss_end
    la t0, _bss_start
    la t1, _bss_end
.Lzero_loop:
    bge t0, t1, .Lzero_done
    sd zero, 0(t0)
    addi t0, t0, 8
    j .Lzero_loop
.Lzero_done:

    // 3. 直接调用 kernel_main (a0 寄存器中已由 Bootloader 填入标准的 dtb_ptr)
    call kernel_main
