# CapsuleOS - Agent Instructions

## 项目概述

**CapsuleOS** 是一个从零构建的、基于微内核架构的现代化类 Unix 操作系统，代号 **Pangu**（开天辟地）。
- 内核: HNX 微内核 (hnxcore.ohc)
- 架构: `aarch64-unknown-none` / `riscv64imac-unknown-none-elf` (多架构支持，软浮点 ABI 对齐)
- 用户态目标三元组: `aarch64-unknown-capsule` / `riscv64-unknown-capsule`
- 语言: Rust only (no_std 裸机内核 + hnxstd 用户空间)

## 二进制格式规范

### 1. 通用多段 `.ohc` 格式 (Multi-Segment OHC Specification)
CapsuleOS 抛弃了用户态直接装载复杂、冗余 ELF 格式的传统做法，采用完全自研、极致轻量的通用 **OHC 胶囊格式**（用于内核镜像、用户态独立程序与共享库）。

```
+------------------------------------+
|  Magic (4B)                        |  0x4F484300 ("OHC\0")
+------------------------------------+
|  Version (2B)                      |  0x0001
+------------------------------------+
|  Entry (8B)                        |  程序入口点虚拟地址 (Entry Point Virtual Address)
+------------------------------------+
|  Segment Count (2B)                |  物理段描述符数量 (如 .text, .rodata, .data/.bss)
+------------------------------------+
|  Flags (2B)                        |  属性标志：0=内核镜像, 1=用户态独立进程, 2=动态共享库 OHLIB
+------------------------------------+
|  Size (4B)                         |  Payload 实际数据大小 (不含 Header 及段描述符)
+------------------------------------+
|  Checksum (4B)                     |  CRC32 校验和 (仅对 Payload 计算)
+------------------------------------+
|  Segment Descriptors               |  段描述数组 [Segment Descriptor; Segment Count]
|  (每个描述符 24 字节)                |  ├─ VirtAddr (8B): 虚拟地址映射起点 (如 0x1000)
|                                    |  ├─ FileOffset (8B): 对应 Payload 数据区偏移量 (如 120)
|                                    |  ├─ Size (4B): 实际数据大小
|                                    |  └─ Flags (4B): 映射权限位 (1=R, 2=W, 4=X)
+------------------------------------+
|  Payload Data                      |  物理存储的代码段与数据段载荷
+------------------------------------+
```

### 2. 启动与装载流程
```
[ 物理世界：裸机启动阶段 ]
QEMU (-kernel capsule-bootloader)
    ↓
capsule-bootloader (子模块, 解析 OHC 头部 -> 校验 CRC32 -> 拷贝扁平 Payload 至 entry)
    ↓
hnxcore.ohc (内核运行, entry = 0x40080000 on aarch64 / 0x80080000 on riscv64)
    ↓
[ 虚拟世界：开启 MMU 4级页表翻译 ]
kernel_main(dtb_ptr) (FDT 驱动自适应匹配绑定 -> 建立 VMAR / VMO 地址管理体系)
    ↓
[ 用户空间：系统服务与应用进程加载 ]
loader (用户态 ELF ➔ 多段 OHC 转换服务 ➔ 解析 OHC 多段头 ➔ 动态创建多段 VMO 并映射入新进程 VMAR ➔ 调度执行)
```

## 🛠️ 工具链与 hnxstd 桥接原理

CapsuleOS 采用自研 `hnxstd` 标准库，在 `no_std` 环境下提供核心数据结构与 I/O 接口：

1. **目标配置文件**：项目在 `std/targets/` 目录下维护自定义目标描述文件 `aarch64-unknown-capsule.json` 与 `riscv64-unknown-capsule.json`，`os = "none"`, `env = "capsule"`。
2. **核心系统调用封装 (`hnxlibc`)**：`userspace/hnxlibc` 通过 `#[no_mangle] pub extern "C"` 导出 UNIX 兼容标准符号（如 `write`、`read`、`open`、`exit`）。
3. **自研标准库 (`hnxstd`)**：`userspace/hnxstd` 提供 `Vec`、`String`、`println` 等基础接口，构建于 `hnxlibc` 之上。
4. **OHC 打包转换**：链接输出的标准 ELF 文件，通过 `ohc-tool` 解析其 Program Headers，自动剥离冗余 debug 符号并重构打包为精简的 `.ohc` 胶囊，注入目标存储区间。

## 📁 Workspace 物理结构

```
capsule-os/
├── .cargo/               # Cargo 别名与编译器 `-Z build-std` 联合配置
├── bootloader/           # 📂 (子模块) capsule-bootloader 引导层
├── kernel/               # 📂 (子模块) hnx-core 纯净微内核
│   ├── linker/           # 架构链接脚本 (kernel_aarch64.ld / kernel_riscv64.ld)
│   └── src/              # 内核核心（arch 架构映射, mm 内存分配, drivers 外设）
├── userspace/            # 📂 用户空间整个生态
│   ├── hnxlibc/          # hnxlibc 运行时底座与标准 C-ABI 符号劫持实现
│   ├── hnxstd/           # hnxstd 自研标准库 (Vec, String, println 等)
│   ├── services/         # 常驻基础服务进程 (init 根进程, loader 装载器, vfs 虚拟文件系统)
│   └── programs/         # 用户态普通程序 (shell 控制台)
├── std/                  # 📂 工具链标准目标文件存放区
│   └── targets/          # *.json 目标三元组描述文件
└── tools/                # 📂 研发工具
    └── xtask/            # 纯 Rust 一键式交叉编译、ELF ➔ OHC 打包转换、QEMU 启动引擎
```

## 编译运行快捷指令

```bash
# 1. 编译并以 OHC 模式在 QEMU 运行 AArch64
cargo xtask run --arch aarch64

# 2. 编译并以 OHC 模式在 QEMU 运行 RISC-V 64 (Soft-Float)
cargo xtask run --arch riscv64

# 3. 仅编译不运行
cargo xtask build --arch aarch64

# 4. 检查工具链
cargo xtask check-toolchain --expected-rust 1.96.1
```

## 关键约定与安全

- **No_std 内核 vs hnxstd 用户态**：内核必须是纯净无 `std` 的裸机代码；用户空间 App 使用 `hnxstd` 自研标准库。
- **句柄拦截**：所有的系统调用传参都必须通过 `Handle` 句柄间接索引，任何物理地址指针均不得从用户态直接穿透至内核空间。
- **驱动彻底沙盒化**：外设驱动除极早期串口字符打印外，其余驱动均运行在用户态 `devmgr` 服务控制的独立进程内。

## 验证与检查命令

```bash
# 1. 检查整个 workspace 的健康状况
cargo check --workspace

# 2. 检查 AArch64 内核编译
cargo check --target aarch64-unknown-none -p kernel

# 3. 检查 RISC-V 64 内核编译
cargo check --target riscv64imac-unknown-none-elf -p kernel
```
