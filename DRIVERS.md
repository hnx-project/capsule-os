# 🛸 CapsuleOS 统一驱动框架规范说明书 (DRIVERS.md)

## 1. 🌌 架构概述 (Introduction)
CapsuleOS (Codename: Pangu) 为了平衡 **系统安全性 (Microkernel Isolation)** 与 **高吞吐 I/O 性能 (Hybrid Efficiency)**，设计了双轨制驱动加载框架：
*   **PillsMod (内核态驱动框架 - 对标 macOS kext)**：运行于特权级 EL1，与内核直接绑定或动态链接，拥有对物理硬件、MMU、高速缓存和 DMA 的无限制绝对控制权。
*   **PillsAddon (用户态驱动框架 - 对标 macOS DriverKit)**：运行于 EL0 用户空间沙箱，受 VMAR 虚拟内存管理和进程权限控制（Capability）的双重约束。驱动发生异常时直接进程级自愈，保证微内核核心绝不死机。

---

## 📁 2. 物理目录与生存空间规范 (Directory Layout & Boundary)

根据特权级安全边界与编译目标的物理隔离原则，驱动模块在目录结构上必须遵循如下划分：

```text
.
├── mcore/                        # 📂 特权级 HNX 混合内核核心 (EL1, target: aarch64-unknown-none)
│   └── src/
│       ├── pillsmod/             # 🛠️ [PillsMod (Kext) 核心加载与注册管理器]
│       └── drivers/              # 🔌 内核平台无关驱动抽象与机制 (block/mod.rs 等)
│
├── pillsmod/                     # 📂 [PillsMod 内核态动态驱动模块] (hello_pill, virtio_blk)
│
├── pillsaddon/                   # 📂 [PillsAddon 用户态安全驱动进程] (touchd, gpud, inputd, netd)
│
├── libraries/                    # 📂 基础标准与运行时库 (L2)
│   ├── libcapsule/
│   │   └── src/
│   │       └── pillsaddon/       # 🛠️ [PillsAddon (DriverKit) 用户态安全接口抽象层]
│   └── libc/ / libstd/           # 🧬 C/Rust 标准基础库
│
└── userspace/                    # 📂 用户空间生态 (EL0, target: aarch64-unknown-capsule)
    ├── services/                 # 📂 标准系统服务 (fileagent, procmgr, servicesd)
    └── programs/                 # 📂 用户空间标准应用程序 (osh, ls, cat)
```

---

## 🔌 3. PillsMod (内核态驱动规范 - kext)

### 3.1 职责与生存边界
*   **运行特权**：EL1 (HNX Microkernel 内部)。
*   **适用设备**：高带宽、极低延迟外设（如 VirtIO-Block 磁盘控制器、VirtIO-Net 极速网卡、GIC 中断控制器）。
*   **底层访问**：允许直接使用 AArch64 汇编指令（如 `mrs` / `msr`）进行控制寄存器操作，可直接读写全局物理内存与页表（Page Table）。

### 3.2 生命周期与接口约定
每一个 `PillsMod` 内核驱动模块必须通过标准的动态加载符号接口定义：

```rust
use libpillsmod::KernelImportTable;

/// PillsMod 统一入口点
#[no_mangle]
#[link_section = ".entry"]
pub extern "C" fn pillsmod_init(kernel: &KernelImportTable) -> i32 {
    // 探测设备、建立队列并向内核注册：
    // let ret = (kernel.register_block_device)(&BLOCK_DEVICE_OPS);
    0
}

/// PillsMod 统一卸载点
#[no_mangle]
pub extern "C" fn pillsmod_exit(kernel: &KernelImportTable) -> i32 {
    0
}
```

---

## 🧩 4. PillsAddon (用户态驱动规范 - DriverKit)

### 4.1 职责与生存边界
*   **运行特权**：EL0 (沙箱用户进程)。
*   **适用设备**：高交互性、热插拔、非核心总线外设（如 VirtIO-GPU 显卡、VirtIO-Input 触控板、键盘/鼠标）。
*   **底层隔离**：禁止直接使用特权物理内存访问指令。所有的 MMIO 读写必须通过内核代理。

### 4.2 寄存器安全审计 (MMIO Sandbox)
PillsAddon 驱动在启动时，必须向 `devmgr` (设备管理器) 注册其期望接管的物理基地址和长度。
内核通过 **`SYSCALL_MMIO_READ`** 与 **`SYSCALL_MMIO_WRITE`** 进行寄存器代理，其安全审计流程如下：

```text
[ PillsAddon: gpud ] ──1. mmio_write(reg, val) ──► [ SYSCALL_MMIO ] (EL1 内核)
                                                           │
                                                  2. 审计物理地址 (devmgr 白名单)
                                                           ├── 符合 ──► 写入硬件 ──► 返回 Ok
                                                           └── 越权 ──► 拒绝写入 ──► 返回 AccessDenied (杀掉驱动)
```

### 4.3 中断异步转发机制 (Interrupt Forwarding)
由于 PillsAddon 驱动无法直接承接 CPU 的物理中断向量（VBAR_EL1），硬件中断由内核中断控制器（GIC）捕获后，通过 **Kernel Port (等待队列机制)** 异步转发至用户态：

```text
[ 物理中断信号 ] ──► 1. 触发 GIC 中断 ──► 2. 内核中断处理函数 (ISR)
                                                 │
                                                 ▼
[ 用户态 gpud ]  ◄── 4. 线程唤醒 (Ready) ◄── 3. 内核 Port 投递事件并唤醒等待队列
```

---

## 📊 5. 驱动框架对比矩阵 (Trade-offs Matrix)

| 维度特性 | PillsMod (内核态 kext) | PillsAddon (用户态 DriverKit) |
| :--- | :--- | :--- |
| **执行特权级** | **EL1** (Privileged) | **EL0** (User Sandboxed) |
| **物理目录** | `pillsmod/` | `pillsaddon/` |
| **编译目标** | `aarch64-unknown-none` | `aarch64-unknown-capsule` |
| **崩溃影响面** | 致命 (引起 Kernel Panic) | 隔离 (仅驱动进程崩溃，服务可热重启) |
| **I/O 传输效率**| 极致 (直接 DMA 映射，零拷贝) | 较高 (依赖内核 MMIO 代理与 Port 唤醒) |
| **中断响应延迟**| **0 延迟** (物理中断向量直达) | **低延迟** (依赖内核上下文调度唤醒) |
| **典型代表设备**| 磁盘 (`virtio_blk`)、网卡 (`virtio_net`) | 显卡 (`gpud`)、触控板 (`touchd`) |

---

## 📦 6. `.pillsmod` / `.pill` 单文件分发格式与加载协定 (Binary Bundle Specification)

为了满足高内聚、易于分发、并支持 `xtaskfile` 全自动编译打包流的需求：
```toml
pillsmod = [
    { path = '..../Cargo.toml', output = '{BUILD_TEMP_RESOURCE}/extensions/*.pillsmod' }
]
```
CapsuleOS 制定了单一归档格式的 `.pillsmod` (内核态) 与 `.pill` (用户态) 驱动文件规范。

### 6.1 二进制物理结构 (Binary Layout)

```text
┌─────────────────────────────────────────────────────────────┐
│ 1. 头部魔数区 (Header) - 32 字节                             │
│    - Magic: 4 字节 [0x50, 0x49, 0x4c, 0x4c] ("PILL")        │
│    - Version: 4 字节 (主/次版本号)                          │
│    - Metadata Offset: 8 字节                                │
│    - Metadata Size: 8 字节                                  │
│    - Payload Offset: 8 字节                                 │
│    - Payload Size: 8 字节                                   │
├─────────────────────────────────────────────────────────────┤
│ 2. 元数据区 (Metadata) - TOML / JSON 文本                     │
│    - 包含驱动名称、类别 (PillsMod/PillsAddon)、设备树兼容性、   │
│      MMIO 注册区间、绑定中断号、加载入口符号等信息                │
├─────────────────────────────────────────────────────────────┤
│ 3. 驱动负载区 (Payload) - 纯二进制代码段 (Executable Binary) │
│    - 已经由 ohlink-linker 链接完成的对齐可执行段 (Text/Data)   │
└─────────────────────────────────────────────────────────────┘
```

#### 6.1.1 头部结构体定义 (C-ABI Representation)
```rust
#[repr(C, packed)]
pub struct PillHeader {
    pub magic: [u8; 4],         // 必须为 b"PILL"
    pub version_major: u16,     // 主版本号，例如 1
    pub version_minor: u16,     // 次版本号，例如 0
    pub metadata_offset: u64,   // 描述文本在文件中的绝对偏移
    pub metadata_size: u64,     // 描述文本大小
    pub payload_offset: u64,    // 驱动代码段在文件中的绝对偏移
    pub payload_size: u64,      // 驱动代码段字节大小
}
```

#### 6.1.2 驱动描述文本 (`metadata.toml` / Json) 格式样例
```toml
[driver]
name = "virtio_net"               # 驱动唯一标识符
version = "1.0.0-beta"            # 驱动版本
class = "PillsMod"                # 核心类型: [PillsMod (EL1) | PillsAddon (EL0)]
entry_symbol = "pillsmod_init"    # 驱动加载入口点

[requirements]
kernel_version = ">=1.0.0"        # 内核版本依赖
dependencies = ["pci_bus"]        # 依赖的其他驱动模块

[resources]
mmio_regions = ["0x0a000000/0x200"] # 声明接管的硬件 MMIO 地址区间 (安全审计用)
irqs = [32]                       # 申请绑定的中断向量号
```

---

## 🔄 7. 构建与动态加载流程 (Build & Load Pipeline)

### 7.1 `xtask` 自动化打包流 (Build Orchestration)
1. **编译驱动**：通过 `cargo build --release` 编译驱动（PillsMod 采用 EL1 target，PillsAddon 采用 EL0 target）。
2. **提取负载**：利用 `objcopy -O binary` 从 ELF 提取纯净代码段 `driver.raw`。
3. **元数据集成**：在驱动项目的根目录下自动读取 `metadata.toml` 配置文件。
4. **合成封包**：按 `PillHeader` 规整各个偏移并拼接二进制字节流，输出并写入 `{BUILD_TEMP_RESOURCE}/extensions/name.pillsmod` (或 `.pill`)。

### 7.2 动态加载自举机制 (Dynamic Loading Mechanism)
1. **扫描设备**：自举期间，用户态 `devmgr` 或 UEFI 引导器自动扫描 U 盘中的 `/extensions/*.pillsmod` (或 `.pill`) 文件。
2. **校验解析**：读入内存后，解析 `PillHeader` 校验 `"PILL"` 签名。若为 `PillsAddon`，则就地调用 `sys_spawn` 建立独立进程沙箱并注册 MMIO 地址白名单。
3. **内核级 PillsMod 注册 (Kext Loader)**：
   - 若 class 为 `PillsMod`，则调用专用的内核系统调用。
   - 内核 `pillsmod` 模块（物理目录为 `mcore/src/pillsmod/`）接收该 VMO 物理盘，解析并将其装载到内核高半区虚拟内存（Identity-mapped 或 VMAR 专用段中）。
   - 审计并注册中断向量，提取符号执行跳转自检（`pillsmod_init`）。
