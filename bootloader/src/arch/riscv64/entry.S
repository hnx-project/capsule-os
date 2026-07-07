.section .text.entry
.global _start
_start:
    # 禁用所有中断
    csrw sie, zero

    # 设置栈指针为链接脚本分配的地址
    la sp, _stack_top

    # QEMU / OpenSBI 传入 a0 = hartid, a1 = DTB pointer
    # 将 a1 移动到 a0，使符合 rust_main(dtb_ptr: *const u8) 的 C 语言调用约定
    mv a0, a1
    tail rust_main
