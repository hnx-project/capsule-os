# HNX Core (Microkernel)

```text
   __  ___   ___  __   ______  ____  _____
  / / / / | / / |/ /  / ____/ / __ \/ ___/
 / /_/ /  |/ /|   /  / /     / / / /\__ \ 
/ __  / /|  //   |  / /____ / /_/ /___/ / 
/_/ /_/_/ |_//_/|_|  \____/  \____//____/  
```

> **HNX Core** 是一个从零开始构建的、完全基于 Rust 语言编写的极简高性能微内核（Microkernel），代号 **Pangu** (开天辟地)。
>
> 它是 **CapsuleOS** 操作系统的技术基石，只运行于最高特权级（AArch64 EL1 / RISC-V S-Mode），提供极致精简的内核态抽象。

---

## 🏛️ 微内核设计哲学

HNX Core 遵循严格的**微内核隔离（Privilege Separation）**原则：
*   **内核态只做 3 件事**：
    1.  **进程与线程调度**：极其高效的多核线程上下文切换。
    2.  **进程间通信 (IPC)**：通过 `Channel` 与 `Port` 提供无拷贝的高速数据和句柄（Handles）传递。
    3.  **内存空间抽象**：管理物理页帧（VMO）与虚拟地址区间（VMAR），为用户态提供地址隔离。
*   **其他一切在用户态运行**：文件系统、网络栈、硬件驱动（包含显卡、网卡等）全部运行在低特权级用户空间（EL0 / U-Mode），任何驱动崩溃都不会导致内核挂死，保障系统高可用。

---

## 📂 仓库架构说明

HNX Core 采用 Rust Workspace 组织，不掺杂任何用户态软件，保证内核的绝对纯净。

```text
hnx-core/
├── .cargo/
│   └── config.toml         # 裸机 target 编译 flags
├── hal/                    # 📂 1. 物理硬件抽象层 (no_std, traits only)
├── shared/                 # 📂 2. 内核与用户态共享定义 (Syscalls, Status)
├── src/                    # 📂 3. 核心微内核实现 (100% 扁平化，无冗余嵌套)
│   ├── arch/               # AArch64 / RISC-V 64 核心硬件指令级操作
│   ├── board/              # QEMU Virt 等板级参数适配
│   ├── drivers/            # 极简控制台驱动
│   ├── mm/                 # 物理页帧分配器与页表翻译 (Phys, VMO, VMAR)
│   ├── ipc/                # 句柄与消息通道通信
│   └── task/               # 进程线程调度 (Scheduler, Process, Thread)
├── linker/                 # 📂 4. 架构专属链接脚本 (kernel_aarch64.ld / kernel_riscv64.ld)
├── tools/
│   └── ohc-tool/           # OHC 打包工具 (将 raw kernel 包装为 .ohc 镜像)
└── Cargo.toml
```

---

## 🛠️ 编译与开发验证

由于 **HNX Core** 是专门针对 CapsuleOS 架构设计的微内核，其编译和打包生命周期已完美整合进统一工作区中：

### 1. 独立开发与代码自检 (Standalone)
在 `kernel/` 子目录下进行日常内核代码编写与重构时，可以直接使用标准 Cargo 命令针对不同架构目标三元组进行极速静态类型与编译检查：

```bash
# 检查 AArch64 架构下内核编译健康度
cargo check --target aarch64-unknown-none -p kernel

# 检查 RISC-V 64 架构下内核编译健康度 (软浮点 lp64 ABI 匹配)
cargo check --target riscv64imac-unknown-none-elf -p kernel
```

### 2. 全生命周期构建与 QEMU 启动 (Unified)
内核的物理链接（`rust-lld`）、裸二进制提取（`llvm-objcopy`）、`hnxcore.ohc` 精简多段镜像封装以及 QEMU 一键启动，全部由主工作区根目录的 `cargo xtask` 自动化编译流程进行统一编排管理。

请退回到项目根目录并运行：

```bash
# 1. 编译并以 OHC 模式在 QEMU 运行 AArch64 (默认)
cargo run-ohc

# 2. 编译并以 OHC 模式在 QEMU 运行 RISC-V 64 (Soft-Float)
cargo run-riscv
```
