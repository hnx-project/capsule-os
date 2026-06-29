# CapsuleOS 开发计划 (与时俱进整理版)

> 目标: 快速出成果，结合 capsule-bootloader 现代特性，先通后精。
> 构建与运行采用纯 Rust 极简原生体系：`cargo run-ohc` 与 `cargo run-riscv`。

---

## 🟢 Phase 1: v0.1.0 Pangu - 可启动内核与引导对接 (已圆满完成) ✅

> 目标: hnxcore.ohc 能够通过 capsule-bootloader 安全加载，动态初始化栈和 BSS，接收 DTB 指针并打印 "CapsuleOS v0.1.0" 与 "OK"

### 1.1 OHC 包打包工具 (`ohc-tool` -> 集成至 `xtask`) ✅
- [x] tools/xtask: 实现 OHC 头部写入、CRC32 校验、Payload 打包
- [x] xtask: 集成一键将内核 Raw Binary 打包为 `.ohc` 格式并实现全生命周期启动管理

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
- [x] `lib.rs`: 移除硬编码的 inline 汇编字符打印，使用统一 of `arch::console_putchar` 输出标准 boot 消息
- [x] `cargo run-ohc`: 完整联调测试，Bootloader 正确加载解包并平滑跳转至 CapsuleOS 打印成功

---

## 🟡 Phase 2: v0.2.0 Pangu - 动态外设探测与内存管理 ✅

> 目标: 激活 MMU 4级页表建立段/页级映射，实现物理内存管理与虚拟地址空间管理 (VMAR/VMO)

### 2.1 动态外设与内存范围探测 (FDT 联动) ✅
- [x] 引用并编写轻量 FDT 树解析，基于全局 `DTB_POINTER` 动态定位物理 RAM 的首尾物理地址（替换硬编码内存范围）
- [x] 动态定位 chosen 标准控制台 PL011 / NS16550 的 MMIO 基地址（为多板支持解耦硬编码 `0x09000000`）

### 2.2 物理页分配器 (Physical Page Allocator) ✅
- [x] 实现物理页面管理机制（隐式物理页面空闲链表，0 字节额外元数据，高效 O(1) 分配与回收）
- [x] 实现 `allocate_page()` / `free_page()` 底层原子物理页分配接口

### 2.3 MMU 分页系统与页表管理 (AArch64 / RISC-V SV39) ✅
- [x] 实现 4 级页表建立（支持 4KB 页），支持设置 `AArch64PageFlags`（内核/用户态、读/写/执行属性、Device 属性）
- [x] 建立内核早期虚拟地址映射：
  - 恒等映射（Identity Mapping）覆盖 UART 与 kernel/RAM 段
  - 高半核映射 `KERNEL_OFFSET = 0xFFFF_8000_0000_0000`（避开与恒等 1GB Block 的 L1 index 冲突）
  - PL011 MMIO 区间走恒等映射 of Device 1GB Block
- [x] 激活 MMU（配置 `SCTLR_EL1`，`TCR_EL1`，`MAIR_EL1` 和 `TTBRx_EL1` 寄存器并刷新 TLB；TLBI VMALLE1 + DSB SY + ISB）
- [x] AArch64 `cargo run-ohc` 全链路验证通过：MMU 开启，虚拟地址运行通畅。
- [x] RISC-V 64 SV39 路径实现（`kernel/src/arch/riscv64/mmu.rs`）：构建 2 MiB 恒等映射 + L1→L2→4 KiB UART Device 页表。为了对齐 soft-float ABI，统一采用 `riscv64imac-unknown-none-elf` 目标。

### 2.4 虚拟地址空间管理器 (VMAR & VMO) ✅
- [x] **VMO (Virtual Memory Object)**: 真正实现物理页分配与延迟分配（Lazy Allocation）─ 4 KiB granularity；metadata 存于独立 page；API: `create_with_size / commit_page / commit_all / read / write / get_page_phys`；`read` 在 uncommitted page 处返回 0
- [x] **VMAR (Virtual Memory Address Range)**: 真正实现虚拟区间分配、保护属性修改与页表映射关联 ─ 树形结构（root + sub-region）；metadata 存于独立 page；API: `create / allocate_subregion / map / unmap / protect`；`map` 4 KiB 一页一页调 `arch_mmu::map_page` 安装页表项
- [x] **AArch64 4 KiB 页表 + L1/L2 Block shatter**: `arch::aarch64::mmu::map_page` 走 L0→L1→L2→L3；遇 L1 1 GiB Block 或 L2 2 MiB Block 时 **shatter** ─ 分配新的下一级表，把原 512 entries 重写为带原属性位的细粒度映射
- [x] **RISC-V SV39 4 KiB 页表 + L1 Megapage shatter**: `arch::riscv64::mmu::map_page` 走 L1→L2→L3；shatter 2 MiB Megapage 为 L2
- [x] **跨架构统一 MMU 接口**: `arch::mmu` 暴露 `MapFlags` (kernel_rw/ro/rx, user_rw/ro, device_rw) + `map_page / unmap_page / pa_to_kernel_va`
- [x] **跨架构 smoke 验证**: `lib.rs::vmo_vmar_smoke_test()` ─ AArch64 通过 MMU 翻译路径读到 VMO 内容 (MATCH via MMU)；RISC-V 通过 `pa_to_kernel_va` 直接读 VMO 物理页验证 (MATCH via VMO PA)

---

## 🔵 Phase 3: v0.3.0 Pangu - 多任务调度与中断管理

> 目标: 结合 Bootloader 多核状态，完成时钟中断接管、设计进程/线程控制块、建立 Capability 与句柄表，以及优先级时间片轮转（Round-Robin）调度器。

### 3.1 异常处理与时钟中断分发
- [ ] 编写 AArch64 与 RISC-V 64 架构级的异常上下文（`TrapFrame` 保存 31 个通用寄存器与特权级控制寄存器）压栈/出栈汇编代码
- [ ] 接管 GIC (ARM) 与 PLIC (RISC-V 64) 中断控制器，注册硬件定时器（Timer）Tick 中断
- [ ] 完善中断处理向量表（`vector_table`），安全派发时钟中断到 Rust 内核的调度器

### 3.2 进程与线程控制块 (TCB & PCB)
- [ ] 实现线程控制块 `Thread` (TCB)：保存内核栈指针、`TrapFrame` 地址、线程状态（Ready, Running, Blocked, Exited）
- [ ] 实现进程控制块 `Process` (PCB)：关联独立的根虚拟空间 `VMAR`，管理私有 `HandleTable`（句柄表）

### 3.3 句柄表与权限控制 (Handle & Capability Table)
- [ ] 引入 Zircon/seL4 风格的“一切皆对象，对象皆句柄”权限模型
- [ ] 设计 `HandleTable` (句柄表) 支持 Capabilities：通过 u32 索引抽象并管控 VMO、VMAR、Channel、Thread 等内核对象
- [ ] 校验 Syscall 的句柄参数及权限属性（读、写、映射、转移等），彻底隔离物理指针

### 3.4 轮转调度器 (Scheduler)
- [ ] 实现自适应优先级多级反馈队列（MLFQ）或时间片轮转（Round-Robin）调度
- [ ] 编写上下文切换汇编 `switch_to`（保存 `x19-x29` / `s0-s11` 等 callee-saved 寄存器以及 SP, LR）
- [ ] 打通硬件时钟中断，每次 Tick 定时触发 `schedule()` 强行剥夺当前运行线程并切换

---

## 🔵 Phase 4: v0.4.0 Pangu - 高吞吐 IPC 与能力所有权转移

> 目标: 实现进程间高吞吐、零拷贝（或共享内存）的双向 Channel，并在消息传送中安全实现 Capabilities 所有权的跨进程流转。

### 4.1 进程间通信通道 (Channel)
- [ ] 设计 `Channel` 内核对象，包含双端 Endpoint（句柄 A 与句柄 B）
- [ ] 实现 `channel_write()` 与 `channel_read()`：传输纯字节 Payload，由内核暂存并拷贝

### 4.2 句柄/能力跨进程传递 (Handle Transfer via IPC)
- [ ] 实现消息中携带 Handles：一个进程可以通过向 Channel 写入 Handle，将 VMO、VMAR、或另一个 Channel 端点的所有权安全赠予/复制给接收进程
- [ ] 内核在传输期间自动维护源进程 `HandleTable` 的扣除与目标进程 `HandleTable` 的插入，保障内核对象安全传递

### 4.3 异步完成端口 (Port)
- [ ] 实现 `Port` 内核对象，类似 epoll，允许一个线程挂起并异步等待多个 Channel 事件、Timer 事件或内核对象状态转换通知

---

## 🔵 Phase 5: v0.5.0 Pangu - 统一 OHC/std 工具链生态与用户态装载

> 目标: 真正打通用户空间标准 std 桥接，构建基于多段 OHC 格式的高效装载器 loader、系统根服务 init、虚拟文件系统 vfs 与驱动管理器 devmgr。

### 5.1 通用多段 OHC 打包工具 (`ohc-tool` 演进)
- [ ] 扩展 `ohc-tool`：支持解析 Rust/LLVM 链接生成的 ELF Program Headers，提取 `.text`、`.rodata`、`.data` 等段
- [ ] 升级打包协议：构造多段描述符头部，打包为精简、高安全性、防篡改的 Multi-Segment 通用 `.ohc` 胶囊镜像，剥除冗余 debug 符号表

### 5.2 目标三元组与标准 `std` 动态重译编译
- [ ] 创建 `std/targets/aarch64-unknown-capsule.json` 与 `riscv64-unknown-capsule.json` 目标配置文件，激活 `"families": ["unix"]`
- [ ] 在 `hnx-libc` (`userspace/libc`) 中用 `#[no_mangle] pub extern "C"` 完整封装导出 UNIX C-ABI 核心符号（如 `write`、`read`、`nanosleep`、`exit`），桥接劫持 Rust 官方标准库底层系统依赖
- [ ] 配置 `.cargo/config.toml` 中 unstable `build-std` 特性，令上层应用程序（如 `shell`、`init`）能够直接调用 `use std::...` 高层接口编译运行

### 5.3 专用 OHC 加载器 (loader)
- [ ] 实现 `loader` 常驻系统服务：在用户空间直接解析多段 `.ohc` 头部，彻底移除对复杂 ELF 的解析依赖
- [ ] 加载器行为实现：自动为 `.ohc` 中定义的各段分别申请 `VMO`，并按照描述符的虚拟地址和权限标志映射（map）至新进程的 `VMAR`，分配用户态堆栈，并向内核发起运行系统调用

### 5.4 驱动管理器与外设沙盒化 (devmgr & PL011)
- [ ] 彻底移除内核启动后的硬编码外设驱动，将串口驱动移动至用户态独立进程
- [ ] `devmgr` (设备管理器) 运行时解析 DTB 设备树，为 PL011 启动专属的用户态驱动进程
- [ ] 通过特权句柄映射 Device MMIO VMO 到 PL011 进程的 VMAR，直接操控外设寄存器，打通用户态输入输出

---

## 🟣 Phase 6 至 Phase 10 (后续路线图)
- **Phase 6: v0.6.0 POSIX 兼容层** (标准 Syscall 映射、信号、Socket、管道)
- **Phase 7: v0.7.0 文件系统服务** (VFS 双向 Channel 服务，实现 ramfs、FAT16/32 文件系统常驻服务)
- **Phase 8: v0.8.0 网络栈服务** (TCP/IP 协议栈用户态服务，网卡驱动进程)
- **Phase 9: v0.9.0 所有用户态服务集成** (Shell 控制台 + init 守护系统)
- **Phase 10: v1.0.0 完整 capsule-os.img 制作** (基于 OrbisOS GUI 的像素流 VMO 零拷贝共享显示服务器整合)
