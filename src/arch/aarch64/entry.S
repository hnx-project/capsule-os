.section .text.entry
.global _start
_start:
    // ── EL2→EL1 drop ──────────────────────────────────────────────
    // When the CPU starts at EL2 (e.g. via `-device loader`, bypassing
    // PSCI), clear HCR_EL2.IMO/FMO so IRQs/FIQs are NOT routed to
    // EL2.  We drop to EL1 before doing anything else.
    mrs     x15, CurrentEL
    cmp     x15, #(2 << 2)          // EL2 encoded as 0b10 in bits [3:2]
    b.ne    1f
    msr     sctlr_el2, xzr          // Disable EL2 MMU/Cache if active
    msr     cptr_el2, xzr           // Disable all EL2 traps
    isb
    mov     x15, #(1 << 31)         // HCR_EL2.RW = AArch64 at EL1; IMO=FMO=TGE=0
    msr     hcr_el2, x15
    isb
    mov     x15, #0x3C5             // SPSR: EL1h (SP_EL1), all DAIF masked
    adr     x16, 1f
    msr     spsr_el2, x15
    msr     elr_el2, x16
    isb
    eret
1:
    // ── End EL2→EL1 drop; now running at EL1 ──────────────────────
    // 多核启动保护：只允许 Core 0 (主核) 继续执行，其余核心进入 wfi 睡眠
    mrs x1, mpidr_el1
    and x1, x1, #0xff
    cbz x1, .Lprimary_cpu
.Lsecondary_park:
    wfi
    b .Lsecondary_park

.Lprimary_cpu:
    // 关闭中断
    msr daifset, #0xf
    // 启用 FP/SIMD at EL1 (CPACR_EL1.FPEN = 3)
    mov x1, #0x300000
    msr cpacr_el1, x1
    isb

    // 设置栈指针：使用链接脚本计算的安全栈顶，不依赖物理地址硬编码
    ldr x1, =_stack_top
    mov sp, x1

    // 此时 x0 仍保留着 QEMU 传进来的 DTB 物理地址
    b rust_main
