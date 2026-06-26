# CapsuleOS 开发计划 (与时俱进整理版)

> 目标: 快速出成果，结合 capsule-bootloader 现代特性，先通后精

---

## 🟢 Phase 1: v0.1.0 Pangu - 可启动内核与引导对接 (已圆满完成) ✅

> 目标: hnxcore.ohc 能够通过 capsule-bootloader 安全加载，动态初始化栈和 BSS，接收 DTB 指针并打印 "CapsuleOS v0.1.0" 与 "OK"

### 1.1 OHC 包打包工具 (`ohc-tool`) ✅
- [x] tools/ohc-tool: 实现 OHC 头部写入、CRC32 校验、Payload 打包
- [x] Makefile: 集成 `make ohc` 支持一键将内核 Raw Binary 打包为 `.ohc` 格式

### 1.2 capsule-bootloader 引导层对接 ✅
- [x] 集成 capsule-bootloader 子模块，完成多核屏蔽（CPU 0 引导，其余进入 wfi 睡眠）、关闭早期中断及 FPU (FP/SIMD) 初始化
- [x] 实现 Bootloader 对 OHC 格式的安全解析、校验与物理地址解包（`0x4008_0000`）
- [x] 实现了 Bootloader 将 FDT (Device Tree Blob) 物理地址通过 `x0` 寄存器透传给内核

### 1.3 内核底层加载与 C ABI 对齐 ✅
- [x] `boot_asm.S`: 移除硬编码内存地址，**改用链接脚本符号 `__boot_stack_top` 与 `_bss_start` / `_bss_end`** 进行动态物理栈分配与 BSS 段自动清零
- [x] `boot_asm.S`: 接收并暂存 `dtb_ptr` (`x0`) 到 callee-saved 寄存器 (`x19`)，并在调用 `kernel_main` 前恢复到首参寄存器 `x0`
- [x] `kernel_main`: 修改签名，接收 `dtb_ptr` 并将其存入全局静态变量 `DTB_POINTER` 供后续模块使用
- [x] 异常向量表: 加载基础异常向量表（VBAR_EL1 注册），处理 `sync` / `irq` 等基础挂起和桩输出

### 1.4 控制台驱动重构 ✅
- [x] `arch/aarch64/mod.rs`: 实现纯 Rust 的 PL011 早期寄存器级波特率等参数初始化（`early_init()`）
- [x] `lib.rs`: 移除硬编码的 inline 汇编字符打印，使用统一的 `arch::console_putchar` 输出标准 boot 消息
- [x] `make run-ohc`: 完整联调测试，Bootloader 正确加载解包并平滑跳转至 CapsuleOS 打印成功

---

## 🟡 Phase 2: v0.2.0 Pangu - 动态外设探测与内存管理 (当前重点) 🚀

> 目标: 激活 MMU 4级页表建立段/页级映射，实现物理内存管理与虚拟地址空间管理 (VMAR/VMO)

### 2.1 动态外设与内存范围探测 (FDT 联动) ── 🌟 新增
- [x] 引用或编写轻量 FDT 树解析，基于全局 `DTB_POINTER` 动态定位物理 RAM 的首尾物理地址（替换硬编码内存范围）
- [x] 动态定位 chosen 标准控制台 PL011 的 MMIO 基地址（为多板支持解耦硬编码 `0x09000000`）

### 2.2 物理页分配器 (Physical Page Allocator) ✅
- [x] 实现物理页面管理机制（如 Buddy System 伙伴算法、或 Bitmap 分配器）
- [x] 实现 `allocate_page()` / `free_page()` 底层原子物理页分配接口

### 2.3 MMU 分页系统与页表管理 (AArch64 Page Table)
- [x] 实现 4 级页表建立（支持 4KB 页），支持设置 `AArch64PageFlags`（内核/用户态、读/写/执行属性、Device 属性）
- [x] 建立内核早期虚拟地址映射：
  - 恒等映射（Identity Mapping）覆盖 UART 与 kernel/RAM 段
  - 高半核映射 `KERNEL_OFFSET = 0xFFFF_8000_0000_0000`（避开与恒等 1GB Block 的 L1 index 冲突）
  - PL011 MMIO 区间走恒等映射的 Device 1GB Block
- [x] 激活 MMU（配置 `SCTLR_EL1`，`TCR_EL1`，`MAIR_EL1` 和 `TTBRx_EL1` 寄存器并刷新 TLB；TLBI VMALLE1 + DSB SY + ISB）
- [x] AArch64 `make run-ohc` 全链路验证通过：MMU 切换前后 `[KERNEL] HNX v0.2.0-dev` → `[MM] Physical page allocator` → `[MMU] 4-level page tables ACTIVE` → `OK` 完整串行输出
- [x] RISC-V 64 SV39 路径实现（`kernel/src/arch/riscv64/mmu.rs`）：构建 2 MiB 恒等映射 + L1→L2→4 KiB UART Device 页表。`csrw satp` 启用后会触发 QEMU virt + OpenSBI 1.7 下的翻译异常（PC 段 0x8008_xxxx 的 SV39 翻译），根因仍待查；当前先 mark_mmu_active 但**禁用 satp 写入**保留 identity MMU-off 行为。AArch64 真启用通过。

### 2.4 虚拟地址空间管理 (VMAR & VMO)
- [ ] **VMO (Virtual Memory Object)**: 真正实现物理页分配与延迟分配（Lazy Allocation）
- [ ] **VMAR (Virtual Memory Address Range)**: 真正实现虚拟区间分配、保护属性修改与页表映射关联

---

## 🔵 Phase 3: v0.3.0 Pangu - 多任务调度与中断管理

> 目标: 结合 Bootloader 多核状态，完成 GIC 中断控制、时间片Tick时钟、线程创建切换与调度器

### 3.1 真正中断管理器 (Interrupt Handler & GIC)
- [ ] 实现 AArch64 真正的高效异常处理上下文保存与恢复（`TrapFrame` 保存 31 个寄存器）
- [ ] 编写 GIC (Generic Interrupt Controller) 驱动，接管定时器 Tick 中断
- [ ] 完善 `vector_table`，打通中断分发到 Rust 内核 `irq_handler`

### 3.2 进程与线程管理 (Task & Thread)
- [ ] 实现 `Thread` 控制块（状态：Ready, Running, Blocked, Exited）与内核栈分配
- [ ] 实现 `Process` 控制块（共享地址空间 VMAR、句柄表 HandleTable）

### 3.3 调度器 (Scheduler)
- [ ] 实现简单的优先级时间片轮转（Round-Robin）调度算法
- [ ] 实现上下文切换（`switch_to` 汇编保存 `x19-x29`，`sp`，`lr` 等 callee-saved 寄存器）
- [ ] 计时器 Tick 支持：每次时钟中断自动触发调度

---

## 🟣 Phase 4 至 Phase 10 (后续路线图)
- **Phase 4: v0.4.0 IPC** (Channel 创建/读写，Port 消息队列，异步通知)
- **Phase 5: v0.5.0 用户态 devmgr** (用户态驱动框架与设备管理器 devhost)
- **Phase 6: v0.6.0 POSIX 兼容层** (Syscall 映射，标准 I/O 接口支持)
- **Phase 7: v0.7.0 文件系统服务** (VFS 虚拟文件系统、ramfs 内存文件系统)
- **Phase 8: v0.8.0 网络栈服务** (TCP/IP 协议栈服务、网络接口驱动)
- **Phase 9: v0.9.0 所有用户态服务集成**
- **Phase 10: v1.0.0 完整 capsule-os.img 制作**
