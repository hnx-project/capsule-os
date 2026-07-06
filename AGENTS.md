# CapsuleOS - Agent Instructions

## 项目概述

**CapsuleOS** 是一个从零构建的、基于微内核架构的现代化类 Unix 操作系统，代号 **Pangu**（开天辟地）。
- 内核: HNX 微内核 (hnxcore)
- 架构: `aarch64-unknown-none` / `riscv64imac-unknown-none-elf` (多架构支持，软浮点 ABI 对齐)
- 用户态目标三元组: `aarch64-unknown-capsule` / `riscv64-unknown-capsule`
- 语言: Rust only (no_std 裸机内核 + hnxstd 用户空间)

## 二进制格式规范

### 1. 通用多段 `.ohlink` 格式 (Multi-Segment OHLINK Specification)
CapsuleOS 抛弃了用户态直接装载复杂、冗余 ELF 格式的传统做法，采用完全自研、极致轻量的通用 **OHLINK 胶囊格式**（用于内核镜像、用户态独立程序与共享库），在编译和打包时默认不写任何后缀（如输出为 `init`, `devmgr`, `vfs`, `loader` 等）。

```
+------------------------------------+
|  Magic (4B)                        |  0x4F484C4B ("OHLK")
+------------------------------------+
|  Version (2B)                      |  0x0002
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
capsule-bootloader (子模块, 解析 OHLINK 头部 -> 校验 CRC32 -> 拷贝扁平 Payload 至 entry)
    ↓
hnxcore (内核运行, entry = 0x40080000 on aarch64 / 0x80080000 on riscv64)
    ↓
[ 虚拟世界：开启 MMU 4级页表翻译 ]
kernel_main(dtb_ptr) (FDT 驱动自适应匹配绑定 -> 建立 VMAR / VMO 地址管理体系)
    ↓
[ 用户空间：系统服务与应用进程加载 ]
loader (用户态 ELF ➔ 多段 OHLINK 转换服务 ➔ 解析 OHLINK 多段头 ➔ 动态创建多段 VMO 并映射入新进程 VMAR ➔ 调度执行)
```

## 🛠️ 工具链与 hnxstd 桥接原理

CapsuleOS 采用自研 `hnxstd` 标准库，在 `no_std` 环境下提供核心数据结构与 I/O 接口：

1. **目标配置文件**：项目在 `std/targets/` 目录下维护自定义目标描述文件 `aarch64-unknown-capsule.json` 与 `riscv64-unknown-capsule.json`，`os = "none"`, `env = "capsule"`。
2. **核心系统调用封装 (`hnxlibc`)**：`userspace/hnxlibc` 通过 `#[no_mangle] pub extern "C"` 导出 UNIX 兼容标准符号（如 `write`、`read`、`open`、`exit`）。
3. **自研标准库 (`hnxstd`)**：`userspace/hnxstd` 提供 `Vec`、`String`、`println` 等基础接口，构建于 `hnxlibc` 之上。
4. **OHLINK 打包转换**：链接输出的标准 ELF 文件，通过 `ohc-tool` 解析其 Program Headers，自动剥离冗余 debug 符号并重构打包为精简的无后缀 **OHLINK** 胶囊，注入目标存储区间。

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
    └── xtask/            # 纯 Rust 一键式交叉编译、ELF ➔ OHLINK 打包转换、QEMU 启动引擎
```

## 编译运行快捷指令

通过 `./install_xtask` 一键安装后，可直接在系统的任何位置运行 `xtask` 开发指令：

```bash
# 1. 编译并以 OHLINK 模式在 QEMU 运行 AArch64
xtask os run --arch aarch64

# 2. 编译并以 OHLINK 模式在 QEMU 运行 RISC-V 64 (Soft-Float)
xtask os run --arch riscv64

# 3. 仅编译不运行
xtask os build --arch aarch64

# 4. 检查交叉编译环境
xtask os check-env --expected-rust 1.96.1
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

## 👥 角色开发与 GitCode 提交规范 (xtask repo)

为了保持最高品质的代码质量、防范版本冲突与子模块混乱，**所有 AI Agent 与开发者提交代码必须强制通过 `xtask` 自动化流程**：

### 1. 基础环境一键安装
在根目录下运行安装脚本，直接编译 release 版本的 `xtask` 并安装至用户的本地 PATH 中，方便后续运行：
```bash
./install_xtask
```

### 2. 贡献者角色工作流 (Contributor Mode)
普通贡献者所有的开发应在其个人的 Fork 仓库上进行：
- **第一步：一键配置 Fork 远端关系**（自动级联转换 `kernel` / `bootloader` 子模块）：
  ```bash
  xtask repo setup-fork --username <您的GitCode用户名>
  ```
- **第二步：本地安全提交**（包含双平台静默编译与版本重名校验防御）：
  ```bash
  xtask repo commit
  ```
- **第三步：一键安全备份到个人 Fork 仓库（级联子模块推送）**：
  ```bash
  xtask repo push
  ```
- **第四步：一键从个人 Fork 仓库拉取最新备份**：
  ```bash
  xtask repo pull
  ```
- **第五步：一键安全变基与 Squash 合并 PR**（强约束：只向 upstream 的 `develop` 分支提交）：
  ```bash
  xtask repo pr
  ```

### 3. 管理员角色工作流 (Administrator Mode)
管理员本地直接在官方主仓库开发：
- **本地开发提交**：`xtask repo commit` 会自动判定管理员身份，**自动豁免版本未递增拦截**。
- **发布生产 MR**：一键发起指向 `main` 生产分支的合并请求：
  ```bash
  xtask repo pr --release
  ```
- **一键版本发布**：强力运行双物理平台交叉编译校验、自适应打包符合命名空间的 ZIP 固件包、自动向 GitCode API 创建 Release（并在远端上游主干**全自动打上对应的 SemVer 标签**）并挂载附件：
  ```bash
  xtask repo release <VERSION>  # 示例：xtask repo release v0.6.0
  ```
