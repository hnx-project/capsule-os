# CapsuleOS - Agent Instructions

## 项目概述

**CapsuleOS** 是一个从零构建的、基于微内核架构的现代化类 Unix 操作系统，代号 **Pangu**（开天辟地）。
- 内核: HNX 微内核 (hnxcore.ohc)
- 架构: `aarch64-unknown-none` / `riscv64imac-unknown-none-elf` (多架构支持，软浮点 ABI 对齐)
- 语言: Rust only (no_std)

## 版本路线图

| 版本 | 代号 | 里程碑 |
|------|------|--------|
| v0.1.0 | Pangu | hnxcore.ohc 能启动并平滑输出 boot 字符 |
| v0.2.0 | Pangu | 内存管理 (MMU 开启、物理页帧、VMO & VMAR 虚实地址管理机制) |
| v0.3.0 | Pangu | 多任务调度 (中断接管、TCB/PCB、HandleTable、Round-Robin 调度器) |
| v0.4.0 | Pangu | IPC 进程间通信 (Channel、端口 Port 与 Capabilities 跨进程转移) |
| v0.5.0 | Pangu | 用户态服务治理 (init、loader、vfs、devmgr 驱动沙盒化集成) |
| v0.6.0 | Pangu | POSIX 兼容层 |
| v0.7.0 | Pangu | 文件系统服务 |
| v0.8.0 | Pangu | 网络栈服务 |
| v0.9.0 | Pangu | 所有用户态服务集成 |
| v1.0.0 | TBD | capsule-os.img 完整系统 |

## 二进制格式

### .ohc 格式 (内核镜像)
```
+------------------+
|  Magic (4B)     |  0x4F484300 ("OHC\0")
+------------------+
|  Version (2B)   |  0x0001
+------------------+
|  Entry (8B)     |  入口点虚拟地址
+------------------+
|  Flags (2B)     |  保留
+------------------+
|  Size (4B)      |  payload 大小
+------------------+
|  Checksum (4B)  |  CRC32
+------------------+
|  Reserved (4B)  |
+------------------+
|  Payload         |  内核代码
+------------------+
```

### 启动流程
```
QEMU (-kernel capsule-bootloader)
    ↓
capsule-bootloader (submodule, 解析 OHC + 拷贝至 entry point)
    ↓
hnxcore.ohc (内核, entry = 0x40080000 on aarch64 / 0x80080000 on riscv64)
    ↓
boot_asm.S: DTB ptr -> x19/a1, 动态栈/BSS 初始化
    ↓
kernel_main(dtb_ptr) (lib.rs)
    ↓
根据 DTB 中的 compatible 属性匹配并动态加载对应的串口驱动（pl011 / ns16550）
    ↓
打印: "CapsuleOS v0.2.0-dev" + "OK"
```

> **重要**: 统一通过 capsule-bootloader 加载 OHC 镜像，坚决废除 Makefile 体系，改用 Rust 原生 `cargo xtask` 进行一键生命周期构建管理。

## 编译运行快捷指令

```bash
# 1. 编译并以 OHC 模式在 QEMU 运行 AArch64 (默认)
cargo run-ohc

# 2. 编译并以 OHC 模式在 QEMU 运行 RISC-V 64 (Soft-Float)
cargo run-riscv
```

## Workspace 物理结构

```
capsule-os/
├── .cargo/               # Cargo 别名与快捷构建指令
├── bootloader/           # 📂 (子模块) capsule-bootloader 引导层
├── kernel/               # 📂 (子模块) hnx-core 纯净微内核
│   ├── linker/           # 架构链接脚本 (kernel_aarch64.ld / kernel_riscv64.ld)
│   └── src/
│       ├── arch/           # CPU 架构级核心逻辑
│       │   ├── aarch64/    # ARM64 CPU 级逻辑、MMU 分页与引导汇编
│       │   └── riscv64/    # RISC-V 64 CPU 级逻辑、MMU 分页与引导汇编
│       ├── drivers/        # 动态匹配外设驱动层 (pl011 / ns16550 串口)
│       └── mm/             # 内存管理层 (含物理页帧分配器、VMO 与 VMAR 虚实地址管理)
├── userspace/            # 📂 用户空间
│   ├── libc/             # hnx-libc 运行时底座与 Syscall 封装
│   ├── services/         # 常驻基础服务进程 (init 根进程, loader 装载器, vfs 虚拟文件系统)
│   └── programs/         # 用户态普通程序 (shell 控制台)
└── tools/                # 📂 研发工具
    └── xtask/            # 纯 Rust 一键式交叉编译、打包 OHC、调度 QEMU 运行引擎
```

## 关键约定

### 驱动设计
- 驱动接口通过 `kernel/src/drivers/` 驱动层注册，禁止引入硬编码外设地址。
- 引导期通过 `fdt` 动态匹配外设 MMIO 地址，后期驱动彻底沙盒化转移至用户态 `devmgr` 服务中运行。

### No_std 约定
- 内核、引导程序及所有的用户态程序和基础服务皆为 `#![no_std]` 裸机程序。
- 用户态程序入口点统一为 `_start()`。

### 内核入口点
- `_start()` 在 `kernel/src/lib.rs` (由 boot_asm.S 引导初始化后跳转)。
- 内核中必须有且仅有一个：`#[panic_handler] fn panic(info: &PanicInfo) -> !`。

### 错误处理
- 统一使用 `shared::status::Status` 枚举。
- 返回值签名对齐：`shared::status::Result<T> = core::result::Result<T, Status>`。

## 验证与检查命令

```bash
# 1. 检查整个 workspace 的健康状况
cargo check --workspace

# 2. 检查 AArch64 内核编译
cargo check --target aarch64-unknown-none -p kernel

# 3. 检查 RISC-V 64 内核编译
cargo check --target riscv64imac-unknown-none-elf -p kernel
```
