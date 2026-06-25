# CapsuleOS - Agent Instructions

## 项目概述

**CapsuleOS** 是一个从零构建的微内核操作系统，代号 **Pangu**（开天辟地）。
- 内核: HNX 微内核 (hnxcore.ohc)
- 架构: aarch64-unknown-none
- 语言: Rust only (no_std)

## 版本路线图

| 版本 | 代号 | 里程碑 |
|------|------|--------|
| v0.1.0 | Pangu | hnxcore.ohc 能启动打印 "CapsuleOS v0.1.0" |
| v0.2.0 | Pangu | 内存管理 (MMU 分页) |
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
hnxcore.ohc (内核, entry = 0x40080000)
    ↓
boot_asm.S: DTB ptr -> x19, 动态栈/BSS 初始化
    ↓
kernel_main(dtb_ptr) (lib.rs)
    ↓
打印: "CapsuleOS v0.1.0" + "OK"
```

> **重要**: 不再使用 stage1 直接引导。统一通过 capsule-bootloader 加载 OHC 镜像。

## 构建命令

```bash
# 编译内核
cargo build --target aarch64-unknown-none -p kernel --release

# 链接 ELF
rust-lld -flavor gnu -T kernel/kernel.ld libkernel.a -o kernel.elf

# 打包 .ohc (使用 ohc-tool)
cargo run -p ohc-tool -- pack --input kernel.raw --output hnxcore.ohc --entry 1074266112

# 快速构建 (make ohc)
make ohc

# 运行 (bootloader + OHC 流程)
make run-ohc

# 编译 capsule-bootloader
make bootloader

# 打包完整系统 (v1.0.0)
cargo run -p capsule-pack -- --kernel hnxcore.ohc --rootfs rootfs/ --output capsule-os.img
```

## Makefile 目标

```bash
make build          # 编译 kernel
make kernel        # 编译 + 链接 ELF
make ohc           # 生成 hnxcore.ohc
make run           # 在 QEMU 运行
make bootloader    # 编译 capsule-bootloader
make run-ohc       # 通过 bootloader 启动 OHC
make clean         # 清理
make check         # 检查编译
```

## 目标三元组

- Kernel: `aarch64-unknown-none` (bare metal, no_std)
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
│       └── arch/aarch64/
│           ├── boot_asm.S  # 启动汇编 (动态栈/BSS 初始化)
│           ├── mod.rs      # 平台初始化 + UART
│           └── mmu.rs      # MMU 初始化 ⭐ 待实现
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
- 实现放在 `kernel/src/arch/<arch>/`
- 禁止在 `hal/` 中实现

### No_std 约定
- `hal/`, `shared/`, `kernel/` 都是 `#![no_std]`
- 用户态程序也是 `#![no_std]`
- 入口点: `_start()`

### 内核入口点
- `_start()` 在 `kernel/src/lib.rs`
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
