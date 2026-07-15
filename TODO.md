# CapsuleOS 开发计划 (与时俱进整理版)

> 目标: 快速出成果，结合 capsule-bootloader 现代特性，先通后精。
> 构建与运行采用纯 Rust 极简原生体系：`cargo xtask code build` 与 `cargo xtask code run`。

---

## 🟢 Phase 1: v0.1.0 Pangu - 可启动内核与引导对接 (已圆满完成) ✅

> 目标: hnxcore.ohc 能够通过 capsule-bootloader 安全加载，动态初始化栈和 BSS，接收 DTB 指针并打印 "CapsuleOS v0.1.0" 与 "OK"

### 1.1 OHC 包打包工具 (`ohc-tool` -> 集成至 `xtask`) ✅
- [x] tools/xtask: 实现 OHC 头部写入、CRC32 校验、Payload 打包
- [x] xtask: 集成一键将内核 Raw Binary 打包为 `.ohc` 格式并实现全生命周期启动管理

### 1.2 capsule-bootloader 引导层对接 ✅
- [x] 集成 capsule-bootloader 引导层，完成多核屏蔽（CPU 0 引导，其余进入 wfi 睡眠）、关闭早期中断及 FPU (FP/SIMD) 初始化
- [x] 实现 Bootloader 对 OHLK 格式的安全解析、校验与物理地址解包（`0x4008_0000`）
- [x] 实现了 Bootloader 将 FDT (Device Tree Blob) 物理地址通过 `x0` 寄存器透传给内核

### 1.3 内核底层加载与 C ABI 对齐 ✅
- [x] `boot_asm.S`: 移除硬编码内存地址，**改用链接脚本符号 `__boot_stack_top` 与 `_bss_start` / `_bss_end`** 进行动态物理栈分配与 BSS 段自动清零
- [x] `boot_asm.S`: 接收并暂存 `dtb_ptr` (`x0`) 到 callee-saved 寄存器 (`x19`)，并在调用 `kernel_main` 前恢复到首参寄存器 `x0`
- [x] `kernel_main`: 修改签名，接收 `dtb_ptr` 并将其存入全局静态变量 `DTB_POINTER` 供后续模块使用
- [x] 异常向量表: 加载基础异常向量表（VBAR_EL1 注册），处理 `sync` / `irq` 等基础挂起和桩输出

### 1.4 控制台驱动重构 ✅
- [x] `arch/aarch64/mod.rs`: 实现纯 Rust 的 PL011 早期寄存器级波特率等参数初始化（`early_init()`）
- [x] `lib.rs`: 移除硬编码的 inline 汇编字符打印，使用统一 of `arch::console_putchar` 输出标准 boot 消息
- [x] `cargo xtask code run`: 完整联调测试，Bootloader 正确加载解包并平滑跳转至 CapsuleOS 打印成功

---

## 🟢 Phase 2: v0.2.0 Pangu - 动态外设探测与内存管理 (已圆满完成) ✅

> 目标: 激活 MMU 4级页表建立段/页级映射，实现物理内存管理与虚拟地址空间管理 (VMAR/VMO)

### 2.1 动态外设与内存范围探测 (FDT 联动) ✅
- [x] 引用并编写轻量 FDT 树解析，基于全局 `DTB_POINTER` 动态定位物理 RAM 的首尾物理地址（替换硬编码内存范围）
- [x] 动态定位 chosen 标准控制台 PL011 / NS16550 的 MMIO 基地址（为多板支持解耦硬编码 `0x09000000`）

### 2.2 物理页分配器 (Physical Page Allocator) ✅
- [x] 实现物理页面 management 机制（隐式物理页面空闲链表，0 字节额外元数据，高效 O(1) 分配与回收）
- [x] 实现 `allocate_page()` / `free_page()` 底层原子物理页分配接口

### 2.3 MMU 分页系统与页表管理 (AArch64 / RISC-V SV39) ✅
- [x] 实现 4 级页表建立（支持 4KB 页），支持设置 `AArch64PageFlags`（内核/用户态、读/写/执行属性、Device 属性）
- [x] 建立内核早期虚拟地址映射：
  - 恒等映射（Identity Mapping）覆盖 UART 与 kernel/RAM 段
  - 高半核映射 `KERNEL_OFFSET = 0xFFFF_8000_0000_0000`（避开与恒等 1GB Block 的 L1 index 冲突）
  - PL011 MMIO 区间走恒等映射 of Device 1GB Block
- [x] 激活 MMU（配置 `SCTLR_EL1`，`TCR_EL1`，`MAIR_EL1` 和 `TTBRx_EL1` 寄存器并刷新 TLB；TLBI VMALLE1 + DSB SY + ISB）
- [x] AArch64 `cargo xtask code run` 全链路验证通过：MMU 开启，虚拟地址运行通畅。
- [x] RISC-V 64 SV39 路径实现（`kernel/src/arch/riscv64/mmu.rs`）：构建 2 MiB 恒等映射 + L1→L2→4 KiB UART Device 页表。为了对齐 soft-float ABI，统一采用 `riscv64imac-unknown-none-elf` 目标。

### 2.4 虚拟地址空间管理器 (VMAR & VMO) ✅
- [x] **VMO (Virtual Memory Object)**: 真正实现物理页分配与延迟分配（Lazy Allocation）─ 4 KiB granularity；metadata 存于独立 page；API: `create_with_size / commit_page / commit_all / read / write / get_page_phys`；`read` 在 uncommitted page 处返回 0
- [x] **VMAR (Virtual Memory Address Range)**: 真正实现虚拟区间分配、保护属性修改与页表映射关联 ─ 树形结构（root + sub-region）；metadata 存于独立 page；API: `create / allocate_subregion / map / unmap / protect`；`map` 4 KiB 一页一页调 `arch_mmu::map_page` 安装页表项
- [x] **AArch64 4 KiB 页表 + L1/L2 Block shatter**: `arch::aarch64::mmu::map_page` 走 L0→L1→L2→L3；遇 L1 1 GiB Block 或 L2 2 MiB Block 时 **shatter** ─ 分配新的下一级表，把原 512 entries 重写为带原属性位的细粒度映射
- [x] **RISC-V SV39 4 KiB 页表 + L1 Megapage shatter**: `arch::riscv64::mmu::map_page` 走 L1→L2→L3；shatter 2 MiB Megapage 为 L2
- [x] **跨架构统一 MMU 接口**: `arch::mmu` 暴露 `MapFlags` (kernel_rw/ro/rx, user_rw/ro, device_rw) + `map_page / unmap_page / pa_to_kernel_va`
- [x] **跨架构 smoke 验证**: `lib.rs::vmo_vmar_smoke_test()` ─ AArch64 通过 MMU 翻译路径读到 VMO 内容 (MATCH via MMU)；RISC-V 通过 `pa_to_kernel_va` 直接读 VMO 物理页验证 (MATCH via VMO PA)

---

## 🟢 Phase 3: v0.3.0 Pangu - 多任务调度与中断管理 (已圆满完成) ✅

> 目标: 结合 Bootloader 多核状态，完成时钟中断接管、设计进程/线程控制块、建立 Capability 与句柄表，以及优先级时间片轮转（Round-Robin）调度器。

### 3.1 异常处理与时钟中断分发 ✅
- [x] 编写 AArch64 与 RISC-V 64 架构级的异常上下文（`TrapFrame` 保存 31 个通用寄存器与特权级控制寄存器）压栈/出栈汇编代码
- [x] 接管 GIC (ARM) 与 PLIC (RISC-V 64) 中断控制器，注册硬件定时器（Timer）Tick 中断
- [x] 完善中断处理向量表（`vector_table`），安全派发时钟中断到 Rust 内核的调度器

### 3.2 进程与线程控制块 (TCB & PCB) ✅
- [x] 实现线程控制块 `Thread` (TCB)：保存内核栈指针、`TrapFrame` 地址、线程状态（Ready, Running, Blocked, Exited）
- [x] 实现进程控制块 `Process` (PCB)：关联独立的根虚拟空间 `VMAR`，管理私有 `HandleTable`（句柄表）

### 3.3 句柄表与权限控制 (Handle & Capability Table) ✅
- [x] 引入 Zircon/seL4 风格的"一切皆对象，对象皆句柄"权限模型
- [x] 设计 `HandleTable` (句柄表) 支持 Capabilities：通过 u32 索引抽象并管控 VMO、VMAR、Channel、Thread 等内核对象
- [x] 校验 Syscall 的句柄参数及权限属性（读、写、映射、转移等），彻底隔离物理指针

### 3.4 轮转调度器 (Scheduler) ✅
- [x] 实现自适应优先级多级反馈队列（MLFQ）或时间片轮转（Round-Robin）调度
- [x] 编写上下文切换汇编 `switch_to`（保存 `x19-x29` / `s0-s11` 等 callee-saved 寄存器以及 SP, LR）
- [x] 打通硬件时钟中断，每次 Tick 定时触发 `schedule()` 强行剥夺当前运行线程并切换

---

## 🟢 Phase 4: v0.4.0 Pangu - 高吞吐 IPC 与能力所有权转移 (已圆满完成) ✅

> 目标: 实现进程间高吞吐、零拷贝（或共享内存）的双向 Channel，并在消息传送中安全实现 Capabilities 所有权的跨进程流转。

### 4.1 进程间通信通道 (Channel) ✅
- [x] 设计 `Channel` 零分配同步会合（Synchronous Rendezvous）架构，扩展 TCB 传输上下文
- [x] 实现静态先进先出（FIFO）双向等待队列 `send_waiters` 与 `recv_waiters`，规避动态内存分配
- [x] 实现 `channel_write()` 与 `channel_read()`：支持直接从发送端线程内核地址向接收端线程直接进行单次内存拷贝的 Direct Handoff 机制
- [x] 增加多线程会合 IPC 全仿真 Smoke 测试，在硬件时钟强占式多任务环境下完美实现 Blocked 挂起与 Ready 唤醒流转

### 4.2 句柄/能力跨进程传递 (Handle Transfer via IPC) ✅
- [x] 实现独立进程句柄表隔离（Process-specific Handle Table Isolation），使每个测试任务（`init` / `worker`）拥有真正隔离的专属句柄表，终结了全局共享句柄表的历史
- [x] 扩展 `HandleTable` 的能力划扣与安全注入接口 `remove_with_rights` 和 `add`
- [x] 实现零内存分配的跨进程句柄/能力所有权安全转移（Move Semantics）机制，当通道写会合时直接在不同的句柄表之间进行能力的扣除与注入
- [x] 增加多进程隔离 VMO 能力流转 Smoke 仿真测试：发送方 A 创建 VMO 并写入数据后通过 Channel 安全流转给接收方 B。A 的 VMO 被成功注销（Revocation Proof 通关），B 安全注入获得并成功解包读写，多平台 100% 验证通过！

### 4.3 异步完成端口 (Port) ✅
- [x] 设计并实现 `Port` 内核对象，规避内核堆分配，创新采用 **Page-allocated 专用事件环形缓冲区** 与 **TCB 完成包槽位（Thread.port_packet_slot）** 的混合设计，使 `Port` 对象极速缩小到 32 字节，完美消除内存腐蚀和爆栈风险！
- [x] 实现 `Port::wait` 和 `Port::queue` 高性能多路复用会合算法
- [x] 增加多路异步事件多任务 Smoke 仿真：服务器进程 A 无事件在 Port 上被动阻塞，事件发生方 B 异步投递一个 PortPacket，直接实现零分配直弹唤醒，多平台 100% 联调通过！

---

## 🟢 Phase 5: v0.5.0 Pangu - 统一 OHLINK/xtask 工具链生态与微内核装载器自举 (已圆满完成) ✅

> 目标: 将原本脆弱的分散式子模块，彻底升级合并为全新的 **Subtree 单体仓架构**。在微内核中引入我们自研的 `#![no_std]` 强类型 `ohlink-format` 协议，废弃原有硬编码偏移及 ohc-tool 打包黑盒。同时改造 `xtask` 自举编译宿主机工具链，完成闭环。

### 5.1 Subtree 级联多仓合流 ✅
- [x] 彻底注销并清除 legacy 子模块，将 `bootloader`、`kernel` 与宿主机工具链 `tools/ohlink-cc` 全部以 Squashed 干净历史形式并入主干。
- [x] 注册 Subtree 专属的上游 Remote 控制器（`bootloader-up`, `kernel-up`, `ohlink-cc-up`），实现极为便利、整洁的一键增量合并。

### 5.2 OHLINK 强类型微内核装载器 (sys_exec) ✅
- [x] 引入 `#![no_std]` 的 `ohlink-format` 格式，将其无缝整合入 HNX 微内核。
- [x] 重构 `sys_exec` 核心逻辑：利用自研的 `OHLK_Parser` 强类型自动解析段表，按 `Text`、`Data`、`Rodata`、`Bss` 严格安全边界校验。
- [x] **CRC32 & 内存安全**：对新拉起的进程二进制体自动进行多项式校验和校验，杜绝任何溢出及硬编码偏移，支持对齐虚存区间物理页注入。
- [x] **Bootloader 魔数对齐**：修改 bootloader 允许一键启动并兼容 `OHLK` 魔数。

### 5.3 目标三元组与 xtask 深度自举改造 ✅
- [x] 改造 `xtask os build` 任务：自动注入对宿主机 `tools/ohlink-cc` 工具链的静默编译（自举出 `ohlink-linker` 与插件）。
- [x] 用户态应用编译：全自动为 `userspace` 四大基础应用注入 `-Zcodegen-backend` 参数。
- [x] **`xtask` 框架双星并轨升级**：重构其为 `xtask code` (OS 代码自举、编译、QEMU 运行、自检) 与 `xtask repo` (一键子树 Pull、Push、Setup-Fork 与自动 Conventional Commit 创建)。

---

## 🟡 Phase 5.5: v0.5.5 → v0.5.9 EL0 trap resilience hardening (进行中) 🚧

> 目标: 关闭 EL0 trap 路径在用户态程序崩溃 / 异常退出时把整个 kernel 拖进死循环的回归。具体四个 commit,均已落地 `develop` 链路：

### 5.5.1 Sync EL0 路径已就绪 (`b81aa66` + `e961130`)
- [x] `b81aa66` fix(kernel): trap: save/restore sp_el0 + double msr elr_el1 to fix post-spawn EL0-FAULT — 修复 user 进程 spawn 后第一次 stack 访问因 sp_el0 仍为内核栈指针而 Data Abort 的回归
- [x] `e961130` chore(kernel): sync trap frame comments: 192 → 208 bytes (lr + sp_el0 + padding) to match `b81aa66`

### 5.5.2 Scheduler Dead-thread 处理 (`2992bd1`)
- [x] `fix(scheduler): skip Dead threads in pop_next + scan self.threads fallback; gate SCHED-SAME short-circuit on non-Dead prev` — `pop_next` 跳过 Dead + ready queue 空时 fallback 扫 `self.threads` 找 non-Dead,打破"杀 thread 后死循环"

### 5.5.3 sys_exit 路径 (`4aace5e`)
- [x] `fix(syscall): process_exit: mark caller Dead + reschedule to break devmgr-exit SCHED-SAME spin` — 之前 `sys_exit` 是 `loop {}`,任何 user 程序退出后 kernel 都会卡在 `SCHED-SAME` 死循环

### 5.5.4 SError EL0 完整 handler (`bf10109`)
- [x] `feat(arch-aarch64): serror_el0: full handler (save 208-byte TrapFrame + dispatch to aarch64_serror_el0_handler + eret) replacing diagnostic halt` — 替换 4 行 `mov x0, #0xc00; bl diag; b halt` 占位,补全 208-byte TrapFrame save + dispatcher + eret
- [ ] **未验证**: 30s QEMU 跑没复现过 SError。需要 fault-injection harness 验证 `SError-FAULT` log + kill-thread + schedule 路径真的工作(否则只是"代码 ready,行为未确认")

---

## 🟡 Phase 6 至 Phase 10 (后续路线图)
- **Phase 6: v0.6.0 POSIX 兼容层** (标准 Syscall 映射、信号、Socket、管道，以及 libstd 用户态标准库与 ohlink-linker 的深度符号绑定)
  - 子项: **Init anchor respawn** (kernel 侧: pid=1 死时自动 spawn `system/bin/init`,跟 Linux `init=` 行为对齐,解决当前 loader 死后系统空转的 WIP 项)
  - 子项: **RISC-V 64 HAL ownership** (当前 reins 4 个都没标 own,需要补 `riscv64-expert` 第五个 rein 或在 aarch64-expert 里拉 S-Mode CSR / `stvec` / `satp` / PMP / SBI 经验沉淀)
- **Phase 7: v0.7.0 文件系统服务** (VFS 双向 Channel 服务，实现 ramfs、FAT16/32 文件系统常驻服务)
- **Phase 8: v0.8.0 网络栈服务** (TCP/IP 协议栈用户态服务，网卡驱动进程)
- **Phase 9: v0.9.0 所有用户态服务集成** (Shell 控制台 + init 守护系统)
- **Phase 10: v1.0.0 完整 capsule-os.img 制作** (基于 OrbisOS GUI 的像素流 VMO 零拷贝共享显示服务器整合)

---

## 🔍 Kernel Completeness Audit (0.5.9-develop)

> 静态扫描 `kernel/src/**` + `kernel/hal` + `kernel/shared` 之后的
> 完整度盘点,见 [`AUDIT.md`](./AUDIT.md)。8 个 high-severity、~22 个
> medium-severity、~30+ 个 low-severity findings,本节只列需要做
> 决定的"先删除还是先实现"的项目,落到对应 Phase 里执行。

### 优先删除 (dead-code 清理,1.0 前必做)

- [ ] `kernel/hal` 整 crate: `Cpu`/`Mmu`/`PageTable`/`PageFlags`/`AddressSpace`/`InterruptController`/`Timer`/`Console`/`Serial`/`PhysicalMemory` 全部未 use。只剩 `AArch64Cpu` 在 `arch/cpu.rs` 也无 caller。
- [ ] `kernel/src/mm/slab.rs` (114 行,`SlabAllocator` 从未被 call)
- [ ] `kernel/src/sync/primitives.rs` (138 行,`Mutex`/`Semaphore`/`Event` 全 dead)
- [ ] `kernel/src/object/handle.rs` (重复于 `handle_table::Slot`)
- [ ] `kernel/src/vfs/PathResolver` (定义了无 caller)
- [ ] `kernel/src/mm/elf.rs::ElfLoader` (假装校验 ELF,实际 `is_valid` 恒返回 `true`;实际路径用 `ohlink_format::parser`)
- [ ] `kernel/src/kcore/alloc.rs` (已删) + `kcore/debug.rs` (已删) — 已在本审计里删
- [ ] `kernel/src/task/smoke.rs` (165 行,smoke-test 代码,但 `kernel_main` 里那段调用被注释了)
- [ ] `task::scheduler::{tick, current_thread_name}` + `tick_count` 字段

### 优先实现 (1.0 前必做)

- [ ] **H1 RISC-V SV39**: 恢复 `csrw satp` + `sfence.vma`,修 mstatus.MPP / menvcfg 继承问题
- [ ] **H2 RISC-V `translate_user_va`**: 写真的 page-table walk,不要 `Some(va)` 占位
- [ ] **H3 RISC-V EL0 launch**: 在 `launch_user_program_with_argv` 里给 RISC-V 加 TTBR0 swap
- [ ] **H4 `sys_read` stub**: 改成真的 rootfs-backed read (或承认 "VFS 通过 fileagent 走 channel,不通过 sys_read",把 sys_read fd!=0 删掉)
- [ ] **H5 scheduler lock 顺序**: `unlock()` 不应该 `enable_irqs()`,PSTATE 也没保存。要么改成 "lock = 关 IRQ + 取排他锁",要么换成真正的 IRQ-safe 自旋锁
- [ ] **H6 thread park 0x1usize**: 改成 vmar_base + 任意有效 user VA(dead thread 反正不会 eret,只要 elr 合法即可)
- [ ] **H7 entry-VA 多 slot collision**: 改 `user_entry_offset` 公式,按 slot 偏移重定位
- [ ] **H8 VFS skeleton**: 删掉 `vfs::Vnode` 假实现,声明 "VFS = fileagent channel",sys_open 只创建 channel handle 即可

### 🚧 已知更深层阻塞（0.5.9-develop）

- [ ] **RISC-V 引导在 MMU 之前就挂了（不是 H1）** — `qemu-system-riscv64` 跑到
      `Bootloader | Jumping to HNX Kernel...` 之后**完全沉默**：连 `HNX` 第一行 log
      都没打，更看不到 panic。`csrw satp` 此时是注释掉的（走 identity mapping），
      所以跟 AUDIT H1 完全无关。挖了一下 disasm：
      - `boot_asm.S` 里的 `la sp, __boot_stack_top` 被 rust-lld 错编：
        目标 `0x801c9910`，实际 sp 落到 `0x80229910`（**+0x60000 / +384 KiB**）
      - `call kernel_main` 同样被错编：目标 `0x80091b36`，实际跳到
        `0x8009fb36`（**+0xE000 / +56 KiB**，落到 kernel_main 之后的某个函数里）
      - 两条都是 `auipc + addi/jalr` 的 hi 拆分错了（hi 比正确值小，lo 偏负）
      怀疑 `rust-lld` 在 RISC-V `R_RISCV_PCREL_HI20/LO12_I` 拆分上有 bug，
      或 `.text.boot` 里某条 `addi t0, t0, 0x8` 被错误启用压缩重排。
      排查路径：手写最小 `.S + .ld` + `llvm-ld` 复现 hi 拆分错；或换
      `lld`/`riscv64-unknown-elf-ld` 试；或 `kernel_main` 头一行用 Rust 重设 sp
      并把 `call kernel_main` 改 `jal kernel_main`（21-bit 直接偏移绕开拆分）。
      优先级：低。当前 RISC-V 路径**已知是半成品**（AUDIT Headline 已经说了），
      修这个不挡 AArch64 主线 release，留到 RISC-V 恢复专题再开。
      commit ref: `ff4b3a5` 之后我们尝试过 `fence` + `csrw satp`，
      验证时发现引导早就挂了 → 撤回到 `256a8af` 之前状态，**未 commit 任何 B 改动**。

### Syscall 表面补全 (0.7 → 1.0)

- [ ] 18 个 missing handlers: `SYSCALL_CHANNEL_CALL=13`, `PORT_CREATE/WAIT/QUEUE=20/21/22`, `VMO_GET_SIZE=33`, `VMO_SET_SIZE=34`, `VMAR_UNMAP=41`, `VMAR_PROTECT=42`, `THREAD_EXIT=52`, `PROCESS_START=61`, `PROCESS_EXIT=62`, `EVENT_CREATE/SIGNAL/ACK=70/71/72`, `TIMER_CREATE/SET/CANCEL=80/81/82`, `FUTEX_WAIT/WAKE=90/91`
- [ ] `GET_TID=2`, `GET_PID=3` 也缺 dispatch arm
- [ ] 所有 missing handler 现在都返回 `Status::NotAllowed`,EL0 caller 看不出 "不支持" vs "参数错"。建议返回 `Status::NotSupported`(需在 `shared::Status` 加一个 variant)

### 安全 / 并发硬化 (1.0 前)

- [ ] `static mut NEXT_FREE_PAGE` / `END_FREE_PAGE` / `FREE_PAGES_COUNT` / `TOTAL_PAGES_COUNT` / `MMU_ACTIVE` / `FUTEX_TABLE` / `HANDLE_TABLE_LOCK` / `REGISTRY_LOCK` 全部 `static mut` 非原子。SMP 或 IRQ-嵌套场景会脏。
- [ ] `syscall/validation.rs` 是 no-op facade,目前靠 `safe_copy_from_user`/`safe_copy_to_user` 兜底。validation 层要真做地址范围 + 权限位检查,不能只 return `Ok(())`。
- [ ] `KernelObject::duplicate` 只支持 `Vmo`,其他 handle 不能 duplicate
- [ ] 全局 `HANDLE_TABLE_LOCK` 跨所有进程的 HandleTable 共用,多进程会串行化
