# CapsuleOS

> **CapsuleOS** 是一个从零开始构建的、基于微内核架构的现代类 Unix 操作系统，代号 **Pangu** (开天辟地)。
>
> 项目采用先进的**"多仓库级联子模块"**工程结构，将核心微内核、基础系统服务、与商业版图形扩展完全隔离解耦。

---

## ✨ 核心特性

- **双星并轨架构支持**：支持 `aarch64-unknown-none` 与 `riscv64imac-unknown-none-elf` (软浮点 ABI) 裸机双架构，运行时自适应启动。
- **自研通用多段 OHC 胶囊格式**：运行期完全与 GNU/ELF 体系物理脱钩，通过自研 Multi-Segment OHC，将应用程序与共享库剥离冗余 debug 信息打包封装。新进程装载由用户态 `loader` 解析 OHC Segment 描述头，秒级高速映射 `VMO` 入新进程的 `VMAR` 虚拟空间。
- **自研 hnxstd 标准库**：基于自定义 `unknown-capsule` 目标三元组，通过 `hnxlibc` 提供核心系统调用封装，`hnxstd` 实现 `no_std` 环境下的基础数据结构与 I/O 接口。
- **现代化固件移交**：完全兼容类 Unix 标准固件 ABI（FDT 物理地址透传），通过 `bootloader`（固件Shim层）平滑加载运行。
- **动态设备树发现 (FDT Binding)**：内核动态解析 DTB 设备树的 `compatible` 属性，运行时动态实例化并注册外设驱动（如 PL011 与 NS16550 串口），实现与具体开发板平台的完全解耦。
- **零拷贝与延迟页分配**：依靠基于物理页帧管理器的虚拟内存管理（VMO 与 VMAR 机制），提供精细的页级缺页加载与写时复制（CoW）。
- **极简高性能微内核 (HNX)**：内核仅保留线程调度、IPC（进程间通信）、虚实内存管理和 Capability 权限控制四大极简服务，其他文件系统、驱动程序、网络栈全部运行在用户空间。
- **30KB 极限物理内核**：通过 LDD 连接器 `--gc-sections` 垃圾回收技术，彻底扫除死代码，微内核二进制体积极速精简至 **30KB** 级别。

---

## 📂 多项目多仓库级联大格局

本项目生态由以下三个物理隔离、独立开发演进的仓库组成：

```text
/Users/admin/personal/code/
├── hnx-core/               # 🚀 1. 纯净 HNX 微内核仓库 (100% 独立)
│
├── capsule-os/             # 🟢 2. 基础 CLI 操作系统仓库 (Submodule 级联)
│   ├── bootloader/         # (子模块：自 capsule-bootloader 仓库)
│   └── kernel/             # (子模块：自 hnx-core 仓库，无冗余嵌套)
│
└── orbis-os/               # 🟣 3. 商业图形操作系统仓库 (Submodule 级联)
    └── capsule/            # (子模块：自 capsule-os 仓库)
        ├── bootloader/     # (二级子模块)
        └── kernel/         # (二级子模块)
```

---

## 🛠️ 快速上手与运行联调

我们在 `capsule-os` 根目录配置了一键式的纯 Rust `cargo xtask` 自动化构建引擎。只需在终端运行对应的 Cargo 快捷别名，系统将自动递归编译内核、打包 `.ohc` 格式内核包，并启动 QEMU 引导运行：

### 1. 安装开发工具链
```bash
# 1. 安装 QEMU (需要 QEMU >= 7.0)
brew install qemu dtc

# 2. 添加 bare-metal 目标工具链
rustup target add aarch64-unknown-none
rustup target add riscv64imac-unknown-none-elf
```

### 2. AArch64 (ARM 64-bit) 平台编译运行 (默认)
```bash
# 一键编译、打包 OHC 并启动 QEMU 引导
cargo xtask run --arch aarch64
```
**期待冷启动日志**：
```text
[Bootloader] Booting v0.2.0-dev...
[Bootloader] DTB found at fallback addr 0x42000000.
[Bootloader] Valid OHC Image Found!
[Bootloader] Extracting payload to entry point...
[Bootloader] Jumping to HNX Kernel...

[KERNEL] HNX v0.2.0-dev
[FDT] Discovered hardware:
  UART base : 0x9000000
  UART type : pl011
  RAM base  : 0x40000000
  RAM size  : 0x20000000 (512 MB)
[MM] Physical page allocator initialized.
  Free pages : 130841 (511 MB) / 130843 (511 MB)
[MMU] 4-level page tables ACTIVE
OK
```

### 3. RISC-V 64 (Soft-Float) 平台编译运行
```bash
# 一键编译、打包 OHC 并启动 QEMU 引导
cargo xtask run --arch riscv64
```
**期待冷启动日志**：
```text
[Bootloader] Booting v0.2.0-dev...
[Bootloader] DTB parsed successfully at 0x9fe00000.
[Bootloader] Valid OHC Image Found!
[Bootloader] Extracting payload to entry point...
[Bootloader] Jumping to HNX Kernel...

[KERNEL] HNX v0.2.0-dev
[FDT] Discovered hardware:
  UART base : 0x10000000
  UART type : ns16550
  RAM base  : 0x80000000
  RAM size  : 0x20000000 (512 MB)
[MM] Physical page allocator initialized.
  Free pages : 130926 (511 MB) / 130928 (511 MB)
[MMU] 4-level page tables ACTIVE
OK
```

---

## 📈 生态开发演进计划 (Roadmap)

@Include @TODO.md
