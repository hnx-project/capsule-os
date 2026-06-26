.section .text.boot, "ax"
.global _start
.type _start, @function
_start:
    // 1. 多核屏蔽：只允许 mhartid == 0 (Hart 0) 继续执行，其余核心进入 wfi 睡眠
    csrr t0, mhartid
    bnez t0, .Lpark_hart

    // 2. 设置栈指针：利用链接脚本导出的通用符号 __boot_stack_top
    la sp, __boot_stack_top

    // 3. 动态 BSS 清零：利用链接脚本符号 _bss_start 和 _bss_end
    la t0, _bss_start
    la t1, _bss_end
.Lzero_loop:
    bge t0, t1, .Lzero_done
    sd zero, 0(t0)
    addi t0, t0, 8
    j .Lzero_loop
.Lzero_done:

    // 4. 将 dtb_ptr 从 a1 寄存器移动到 a0 (RISC-V ABI 标准参数 1)
    mv a0, a1
    call kernel_main

.Lpark_hart:
    wfi
    j .Lpark_hart
