# CapsuleOS

```text
   ______                     _       ____  _____
  / ____/___ _____  _____  __| |     / __ \/ ___/
 / /   / __ `/ __ \/ ___/ / _  |    / / / /\__ \ 
/ /___/ /_/ / /_/ (__  ) / /_| |   / /_/ /___/ / 
\____/\__,_/ .___/____/  \____|    \____//____/  
          /_/                                    
```

> **CapsuleOS** 是一个从零开始构建的、基于微内核架构的现代类 Unix 操作系统，代号 **Pangu** (开天辟地)。
>
> 项目采用先进的**“多仓库级联子模块”**工程结构，将核心微内核、基础系统服务、与商业版图形扩展完全隔离解耦。

---

## ✨ 核心特性

- **双星并轨架构支持**：支持 `aarch64-unknown-none` 与 `riscv64imac-unknown-none-elf` (软浮点 ABI) 裸机双架构，运行时自适应启动。
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
cargo run-ohc
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
cargo run-riscv
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

### [x] Phase 1: v0.1.0 Pangu - 可启动内核与引导对接 ✅
- [x] 开发 `ohc-tool` 完成 `.ohc` 二进制头部和打包功能
- [x] 开发 `capsule-bootloader` 完成多核屏蔽与 FDT 指针透传
- [x] 对齐 C ABI 接口，实现动态栈与 BSS 分配，完成 PL011/NS16550 字符显示

### [x] Phase 2: v0.2.0 Pangu - 内存管理与虚实页面映射 ✅
- [x] **Phase 2.1**: 通过 FDT 动态定位 RAM 起始地址和 UART MMIO 地址
- [x] **Phase 2.2**: 物理页帧管理器就绪（隐式页面空闲链表，0 字节元数据开销，O(1) 效率）
- [x] **Phase 2.3**: MMU 4级页表与虚实页面映射、开启 AArch64/RISC-V 64 恒等与高半核翻译
- [x] **Phase 2.4**: 虚拟地址空间管理器（VMO 与 VMAR 机制实现，页表映射，Shatter 细化，跨架构验证）

### [ ] Phase 3: v0.3.0 Pangu - 多任务调度与中断管理
- [ ] 实现中断 Trap 寄存器上下文保存/恢复及 GIC/PLIC 中断分发
- [ ] 实现内核级 `HandleTable` (句柄表) 支持 Capabilities
- [ ] 实现 Ready, Running, Blocked 多状态线程（Thread）与进程（Process）控制块
- [ ] 实现多核自适应时钟中断轮转（Round-Robin）调度算法

### [ ] Phase 4: v0.4.0 Pangu - 高吞吐 IPC 与能力所有权转移
- [ ] 实现 Zircon 风格的 IPC 双端 `Channel` 通道
- [ ] 实现消息中携带 Handles 功能，实现内核级的能力所有权跨进程转移
- [ ] 实现 `Port` 完成端口机制，支持多路复用异步事件等待

### [ ] Phase 5: v0.5.0 Pangu - 用户态常驻服务与 ELF 装载器
- [ ] `hnx-libc` 实现基于 VMO/VMAR 页面映射的堆分配器（Malloc）
- [ ] 实现 `loader` 服务：用户态解析 ELF 并将 VMO 段装载，启动新进程
- [ ] 实现 `init` (根服务进程)、`vfs` (虚拟文件系统服务) 与 `devmgr` (设备管理器) 的用户态集成
