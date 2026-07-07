# Capsule Bootloader

Capsule Bootloader 是一个专为 **CapsuleOS** 设计的纯裸机 (Bare-Metal) 引导程序。它由纯 Rust 编写，无标准库 (`no_std`)，且不依赖任何庞大臃肿的第三方引导框架。

它的核心使命只有一个：**在底层硬件初始化完成后，解析 CapsuleOS 专属的 `.ohc` 格式内核包，将其加载到正确的物理内存，并平滑地将系统控制权移交给 CapsuleOS。**

---

## ✨ 核心特性

- **现代架构/平台抽象**：高度解耦的架构抽象（`src/arch/`）和平台抽象（`src/platform/`），极易移植到各种新型 CPU 架构与物理开发板。
- **多平台区分编译**：通过 Rust Cargo Features 支持按平台区分编译（如 `virt` 平台），构建脚本自动挂载对应平台链接脚本。
- **纯裸机安全引导**：使用外部统一汇编入口 `entry.S` 接管早期启动，在安全进入 Rust 生态前完成多核屏蔽、关闭中断及 FPU 初始化。
- **链接期安全栈分配**：不再硬编码物理栈地址，通过链接脚本在 BSS 尾部安全、动态地分配 Bootloader 临时栈（`_stack_top`）。
- **原生格式化打印 (println!)**：封装平台串口，支持原生的 `print!` / `println!` 宏，使得裸机调试和恐慌处理（`PanicInfo` 打印）更加得心应手。
- **自定义 OHC 格式解析**：内置 OHC (Open HNX Capsule) 文件格式解析器，安全校验魔数 (Magic)，精确提取内核载荷。
- **设备树 (FDT) 穿透传递**：保留前置固件 (如 QEMU/Coreboot/OpenSBI) 传入的设备树指针，并原封不动地传递给 CapsuleOS。

---

## 📂 目录结构

```text
capsule-bootloader/
├── .cargo/
│   └── config.toml          # Cargo 默认编译配置 (指定目标平台为 aarch64-unknown-none)
├── src/
│   ├── arch/                # 📂 架构抽象层
│   │   ├── mod.rs           # 统一架构操作暴露 (如 halt)
│   │   ├── aarch64/         # ARM64
│   │   │   ├── mod.rs
│   │   │   └── entry.S      # 引导启动汇编 (多核挂起/栈初始化)
│   │   └── riscv64/         # RISC-V 64
│   │       ├── mod.rs
│   │       └── entry.S      # 引导启动汇编 (栈初始化/DTB 参数对齐)
│   ├── linklds/             # 📂 链接脚本统一管理目录
│   │   ├── aarch64/
│   │   │   └── virt.ld      # aarch64 QEMU Virt 链接脚本
│   │   └── riscv64/
│   │       └── virt.ld      # riscv64 QEMU Virt 链接脚本
│   ├── platform/            # 📂 平台抽象层
│   │   ├── mod.rs           # 串口输出格式化、标准 println!/print! 宏实现
│   │   └── virt/            # QEMU Virt 平台
│   │       └── mod.rs       # 内存常量映射 (OHC_BASE) 与 PL011/NS16550 驱动
│   └── main.rs              # 平台/架构无关的通用 Bootloader 解析与跳转主逻辑
├── build.rs                 # 编译期脚本，根据目标架构和选定 Feature 动态绑定对应链接脚本
└── Cargo.toml               # 项目配置 (提供 "virt" 等平台 Features)
```

---

## 🛠 编译与构建

确保你已安装 Rust 工具链，并添加了对应架构的裸机 Target：

```bash
# aarch64 裸机
rustup target add aarch64-unknown-none

# riscv64 裸机 (若需要)
rustup target add riscv64-unknown-none-elf
```

### 1. 编译默认平台 (QEMU Virt)

在当前目录下，执行常规编译即可。项目会默认启用 `virt` 平台 feature：

```bash
cargo build --release
```

### 2. 指定平台与架构编译

您可以通过 `--features` 手动指定目标编译平台：

```bash
# 编译 aarch64 架构下的 virt 平台
cargo build --release --target aarch64-unknown-none --features virt
```

编译产物将位于：`target/aarch64-unknown-none/release/capsule-bootloader`。

---

## 🚀 运行与集成联调 (QEMU)

Capsule Bootloader 与 CapsuleOS 的内嵌微内核 `.ohc` 胶囊完美配合运行。

### 1. 内存布局约定
在 `aarch64` (QEMU `virt` 机器) 实现中，物理内存布局如下：
- `0x4000_0000`: QEMU RAM 起始地址 (Bootloader 载入与运行起点)
- `_stack_top`  : 由链接脚本分配在 BSS 尾部，作为 Bootloader 的初始安全栈（约 `64KB` 空间）
- `0x4070_0000`: OHC 文件被外部挂载的地址 (存放着 OS 内核包)
- `0x4008_0000`: OHC 解包后，CapsuleOS 内核被释放并运行的目标地址 (`Entry`)

### 2. 推荐方式：工作区一键启动
在 CapsuleOS 统一工程环境下，你不需要任何手动编译或手动编写复杂的 QEMU 参数。只需在项目根目录运行一键编译与启动指令：

```bash
# aarch64 一键编译、构建并启动
cargo run-ohc

# riscv64 一键编译、构建并启动
cargo run-riscv
```

### 3. 手动启动命令 (仅用于 Standalone 调试)
如果作为独立仓库进行开发联调，你可以使用 QEMU 的 `-device loader` 参数将生成的 `.ohc` 注入到指定物理地址，并指定加载 bootloader 镜像运行：

```bash
# 启动 QEMU 并在后台查看输出
qemu-system-aarch64 \
    -M virt \
    -cpu cortex-a72 \
    -m 128M \
    -nographic \
    -kernel target/aarch64-unknown-none/release/capsule-bootloader \
    -device loader,file=../dist/kernel/hnxcore.ohc,addr=0x40700000,force-raw=on \
    -semihosting
```

**预期输出日志：**
```text
[Bootloader] Booting...
[Bootloader] DTB parsed successfully.
[Bootloader] Valid OHC Image Found!
  Version : 0x0001
  Entry   : 0x0000000040080000
  Size    : 0x000519e0 bytes
[Bootloader] Extracting payload to entry point...
[Bootloader] Jumping to CapsuleOS...
CapsuleOS v0.1.0
OK
```

---

## 🧩 开发指南与避坑记录

*   **寄存器写入对齐**：ARM64 平台向内存映射设备 (MMIO，例如 PL011 串口) 写入数据时，必须符合对齐要求。例如向 PL011 的 `UART_BASE` 写入字符时，必须使用 `u32` 强转 (`write_volatile::<u32>`)，使用 `u8` 会被硬件忽略丢弃。
*   **入口安全栈设计**：摒弃在汇编或 Rust 行内代码中强行硬编码诸如 `0x40100000` 栈地址的做法。由 `virt.ld` 通过 `. += 0x10000;` 开辟栈空间并通过加载 `_stack_top` 赋值给 `sp` 寄存器。这样不仅避免了地址越界与踩内存风险，更使代码具备跨平台安全性。
*   **多架构参数适配**：在 RISC-V 64 的 `entry.S` 阶段，通过汇编自动进行了 `mv a0, a1` 操作。这是因为 RISC-V 传递给 bootloader 的 DTB 物理地址位于 `a1`，而 C 调用约定的首个参数为 `a0`，这一转换确保了通用 Rust 函数 `rust_main(dtb_ptr)` 能够以完全一致的代码兼容双架构！
