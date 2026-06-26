# CapsuleOS - Agent Instructions

## 项目概述

**CapsuleOS** 是一个从零构建的微内核操作系统，代号 **Pangu**（开天辟地）。
- 内核: HNX 微内核 (hnxcore.ohc)
- 架构: `aarch64-unknown-none` / `riscv64gc-unknown-none-elf` (多架构支持)
- 语言: Rust only (no_std)

## 版本路线图

| 版本 | 代号 | 里程碑 |
|------|------|--------|
| v0.1.0 | Pangu | hnxcore.ohc 能启动打印 "CapsuleOS v0.1.0" |
| v0.2.0 | Pangu | 内存管理 (MMU 分页 + 物理页分配器) |
| v0.3.0 | Pangu | 多任务调度 |
| v0.4.0 | Pangu | IPC (进程间通信) |
| v0.5.0 | Pangu | 用户态 devmgr 服务 |
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
capsule-bootloader (submodule, 解析 OHC + 拷贝到 entry)
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

> **重要**: 不再使用 stage1 直接引导。统一通过 capsule-bootloader 加载 OHC 镜像。

## 构建命令

```bash
# 编译 AArch64 内核 (默认)
make ohc ARCH=aarch64
make run-ohc ARCH=aarch64

# 编译 RISC-V 64 内核
make ohc ARCH=riscv64
make run-ohc ARCH=riscv64

# 编译 capsule-bootloader (支持多架构)
make bootloader ARCH=aarch64
make bootloader ARCH=riscv64
```

## Makefile 目标

```bash
make build          # 编译 kernel
make kernel        # 编译 + 链接 ELF
make ohc           # 生成 hnxcore.ohc
make run           # 在 QEMU 运行 ELF (直接)
make bootloader    # 编译 capsule-bootloader
make run-ohc       # 通过 bootloader 启动 OHC
make clean         # 清理
make check         # 检查编译
```

## 目标三元组

- Kernel (AArch64): `aarch64-unknown-none` (bare metal, no_std)
- Kernel (RISC-V 64): `riscv64gc-unknown-none-elf` (bare metal, no_std)
- Userspace: `aarch64-unknown-none` (最终目标)

## Workspace 结构

```
capsule-os/
├── hal/                    # Hardware Abstraction Layer (no_std)
│   └── src/               # Traits only
├── shared/                 # 共享类型 (no_std)
│   └── src/
├── kernel/                 # HNX 微内核
│   ├── kernel.ld         # 链接脚本
│   └── src/
│       ├── arch/           # 📂 CPU 架构层
│       │   ├── aarch64/    # ARM64 CPU 级逻辑与引导
│       │   ├── riscv64/    # RISC-V 64 CPU 级逻辑与引导
│       │   └── x86_64/     # x86_64 架构桩
│       ├── board/          # 📂 开发板/主板级初始化与参数路由层
│       ├── drivers/        # 📂 动态匹配外设驱动层 (pl011 / ns16550 串口)
│       └── mm/             # 📂 内存管理层 (含物理页隐式链表分配器)
├── userspace/              # 用户态程序
│   ├── libc/             # C 库
│   └── services/         # 系统服务
├── libs/                   # 基础库
│   ├── libc/             # C 标准库
│   └── libcapsule/       # Capsule 原生 API
├── tools/                  # 开发工具 ⭐
│   ├── ohc-tool/         # .ohc 打包工具 ✅ 已实现
│   └── capsule-pack/      # 系统打包工具 ⭐ 待实现
└── rootfs/               # 根文件系统
```

## 关键约定

### HAL 设计
- Traits 定义在 `hal/`
- 实现放在 `kernel/src/arch/<arch>/` 或 `kernel/src/drivers/`
- 禁止在 `hal/` 中实现

### No_std 约定
- `hal/`, `shared/`, `kernel/` 都是 `#![no_std]`
- 用户态程序也是 `#![no_std]`
- 入口点: `_start()`

### 内核入口点
- `_start()` 在 `kernel/src/lib.rs` (由 boot_asm.S 引导跳转)
- 需要: `#[panic_handler] fn panic(info: &PanicInfo) -> !`

### 错误处理
- `shared::status::Status` 枚举
- `shared::status::Result<T> = core::result::Result<T, Status>`

## Phase 1 实现清单 (v0.1.0) ✅ 已完成

| 任务 | 优先级 | 状态 |
|------|--------|------|
| capsule-bootloader | P0 | ✅ 已实现 |
| ohc-tool | P0 | ✅ 已实现 |
| kernel.ld | P0 | ✅ 已实现 |
| 内核入口 + print | P1 | ✅ 已完成 |
| UART 驱动 | P1 | ✅ 已完成 |
| MMU 初始化 | P2 | 待实现 (Phase 2) |

## QEMU 测试

```bash
# 需要 QEMU >= 7.0
brew install qemu

# 运行 .ohc
make run-ohc

# 运行 ELF (直接)
qemu-system-aarch64 -machine virt -cpu cortex-a57 -nographic -kernel kernel.elf
```

## 已知问题

- capsule-pack 尚未实现
- 用户态程序需要 .ohc 打包支持

## 验证命令

```bash
# 检查 kernel 编译
cargo check --target aarch64-unknown-none -p kernel

# 构建 kernel (生成 libkernel.a)
cargo build --target aarch64-unknown-none -p kernel --release

# 完整 workspace 检查
cargo check --workspace

# ohc-tool 帮助
cargo run -p ohc-tool -- --help

# 检查 .ohc 文件
cargo run -p ohc-tool -- info --input hnxcore.ohc
```
