# CapsuleOS

> **CapsuleOS** 是一个从零开始构建的、基于微内核架构的现代类 Unix 操作系统，代号 **Pangu** (开天辟地)。
>
> 项目采用先进的**"多仓库级联子模块"**工程结构，将核心微内核、基础系统服务、与商业版图形扩展完全隔离解耦。

---

## ✨ 核心特性

- **双星并轨架构支持**：支持 `aarch64-unknown-none` 与 `riscv64imac-unknown-none-elf` (软浮点 ABI) 裸机双架构，运行时自适应启动。
- **自研通用多段 OHLINK 胶囊格式**：运行期完全与 GNU/ELF 体系物理脱钩，通过自研 Multi-Segment OHLINK，将应用程序与共享库剥离冗余 debug 信息打包封装。新进程装载由用户态 `loader` 解析 OHLINK Segment 描述头，秒级高速映射 `VMO` 入新进程的 `VMAR` 虚拟空间。
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

## 👥 角色开发流程规范

为保障 CapsuleOS 核心微内核的纯净与稳定，项目根据开发者的不同角色，定义了严苛的、高度自动化的工作流。所有的 Git 拓扑转换、代码提交、PR 合并请求均由自研的 `xtask` 接管。

请首先在根目录下运行安装脚本，将 `xtask` 编译并安装至您的本地 PATH 中，然后根据您的角色开始开发：
```bash
./install_xtask
```

### 1️⃣ 普通贡献者流程 (Fork-Based Contributor)
普通贡献者**禁止**直接向官方主仓库推送任何分支。所有的开发应在其个人的 Fork 仓库上进行，最终通过 `xtask` 向官方 `develop` 分支提交 Merge Request (PR)。

```text
[ 官方主仓库: hnx-project ] (只读，上游)
       │ ▲ 4. 自动创建 MR (目标: develop)
       │ │
       ▼ │ [ xtask repo pr ]
[ 本地工作区 ] ──────► [ 个人 Fork 仓库: 您的账号 ] (读写，origin)
    1. 写代码         3. 自动 Squash & Push
    2. xtask repo commit
```

* **第一步：一键配置 Fork 拓扑**
  如果您刚刚 `git clone` 了主仓库并写了代码，一键将其无损转换为 Fork 模式：
  ```bash
  xtask repo setup-fork --username <您的GitCode用户名>
  ```
  *(注：该命令会自动将您的 origin 指向您的个人 Fork，将上游 hnx-project 指向 upstream，并全自动级联重定向 `kernel` 与 `bootloader` 子模块。)*

* **第二步：规范化本地提交 (commit)**
  ```bash
  xtask repo commit
  ```
  *(注：系统会自动在本地运行 AArch64 / RISC-V 64 双平台静默编译与格式化校验网关。若本地有代码修改但版本号与上游重名，系统将强制拦截，保障版本唯一性。)*

* **第三步：一键同步上游 (sync)**
  在开发前或合并后，一键同步上游最新的 `develop` 代码和子模块指针：
  ```bash
  xtask repo sync
  ```

* **第四步：一键安全备份到个人 Fork (push)**
  如果您完成了阶段性工作或想进行多端备份，一键将当前分支及所有被修改的子模块级联安全推送到您自己的 Fork 仓库：
  ```bash
  xtask repo push
  ```
  *(注：系统会自动级联探测所有子模块的本地提交状态，全自动把 Dirty 子模块推送到对应子模块的个人 Fork，并在主干完成最新指针绑定与安全推送，全程 100% 自动。同时默认将本地上游追踪设为 `origin/develop`，彻底消灭 IDE 的视觉挂起超前警告。)*

* **第五步：一键拉取个人 Fork 备份 (pull)**
  一键拉取并对齐您在个人远端 Fork 仓库备份的最新代码与子模块状态：
  ```bash
  xtask repo pull
  ```

* **第六步：一键 Squash & 跨仓库创建 MR (pr)**
  ```bash
  xtask repo pr
  ```
  *(注：系统会自动抓取 upstream 最新的 develop 分支并执行强制 rebase 解决冲突。随后，自动软重置（soft-reset）并**将您的所有零散 commits 压缩（Squash）为单一干净的规范提交**，推送至您的 Fork 仓库，最后自动向 hnx-project 的 `develop` 分支发起合并请求。)*

---

### 2️⃣ 核心管理员流程 (Maintainer / Administrator)
核心管理员（拥有主仓库写入权限）主要负责日常主仓库分支的整理、PR 评审，以及向生产分支 `main` 发布版本。

```text
[ 官方主仓库: hnx-project ]
  ├─ develop (日常集成)
  │      │ ▲
  │      ▼ │ [ xtask repo pr --release ]
  └─ main (生产发布) <─── [ xtask repo tag <version> ]
```

* **本地开发与豁免通道**：
  - 管理员的本地 `origin` 远端直接指向官方主仓库（`hnx-project/capsule-os`）。
  - 在运行 `xtask repo commit` 时，系统会自动探测并判断您为管理员，**自动豁免“版本号不得与上游重名”的拦截**，允许您自由地进行发布前的版本微调和维护提交。

* **发布版合并请求 (Release MR)**：
  - 当管理员在 `release/*` 分支上准备将积累的特性合并入生产 `main` 分支时，运行：
    ```bash
    xtask repo pr --release
    ```
    *(注：系统会突破常规开发者只能目标 develop 的限制，自动将 GitCode 合并请求的目标锁定并重定向为 `main` 分支。)*

* **一键打包发布版本 (Release & Upload)**：
  - 只能在 `main` 或 `release/*` 分支上进行发布。运行：
    ```bash
    xtask repo release <VERSION>  # 例如：xtask repo release v0.6.0
    ```
    *(注：系统会首先强力运行双物理架构 Release 最终类型校验。通过后，自动在本地进行全平台 zip 打包并请求 GitCode API ➔ GitCode 平台在后台会自动、无阻碍地在主仓库上生成该 Tag，无需在本地执行任何 Tag 的推送尝试，彻底消灭受保护分支对 Tag 推送的拦截！)*

---

## 🛠️ 快速上手与运行联调

我们在 `capsule-os` 根目录配置了一键式的纯 Rust `xtask` 自动化构建引擎。只需在终端运行对应的 `xtask os` 级联子命令，系统将自动递归编译内核、打包 OHLINK 格式内核包，并启动 QEMU 引导运行：

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
# 一键编译、打包 OHLINK 并启动 QEMU 引导
xtask os run --arch aarch64
```
**期待冷启动日志**：
```text
[Bootloader] Booting v0.2.0-dev...
[Bootloader] DTB found at fallback addr 0x42000000.
[Bootloader] Valid OHLINK Image Found!
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
# 一键编译、打包 OHLINK 并启动 QEMU 引导
xtask os run --arch riscv64
```
**期待冷启动日志**：
```text
[Bootloader] Booting v0.2.0-dev...
[Bootloader] DTB parsed successfully at 0x9fe00000.
[Bootloader] Valid OHLINK Image Found!
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
