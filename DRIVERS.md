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
│       └── drivers/              # 🔌 内核特权级驱动 (virtio_blk, virtio_net 等)
│
├── libraries/                    # 📂 基础标准与运行时库 (L2)
│   ├── libcapsule/
│   │   └── src/
│   │       └── pillsaddon/       # 🛠️ [PillsAddon (DriverKit) 用户态安全接口抽象层]
│   └── libc/ / libstd/           # 🧬 C/Rust 标准基础库
│
└── userspace/                    # 📂 用户空间生态 (EL0, target: aarch64-unknown-capsule)
    ├── services/                 # 📂 标准系统服务 (fileagent, procmgr, servicesd)
    └── addons/                   # 📂 [PillsAddon 用户态安全驱动进程] (touchd, gpud, inputd)
```

---

## 🔌 3. PillsMod (内核态驱动规范 - kext)

### 3.1 职责与生存边界
*   **运行特权**：EL1 (HNX Microkernel 内部)。
*   **适用设备**：高带宽、极低延迟外设（如 VirtIO-Block 磁盘控制器、VirtIO-Net 极速网卡、GIC 中断控制器）。
*   **底层访问**：允许直接使用 AArch64 汇编指令（如 `mrs` / `msr`）进行控制寄存器操作，可直接读写全局物理内存与页表（Page Table）。

### 3.2 生命周期与接口约定
每一个 `PillsMod` 内核驱动模块必须通过标准的静态宏注册或动态加载入口定义：

```rust
// PillsMod 核心生命周期特质
pub trait PillsMod {
    /// 驱动模块初始化函数。
    /// 内核引导或模块加载时首个调用，负责设备树 (FDT) 探测与 MMIO 物理分配。
    fn pillsmod_init(&self) -> Result<(), Status>;

    /// 驱动模块注销与卸载清理函数。
    /// 负责释放分配的物理页面（kstack）、解绑中断向量。
    fn pillsmod_exit(&self) -> Result<(), Status>;
}

// 统一注册宏
#[macro_export]
macro_rules! register_pillsmod {
    ($mod_type:ty) => {
        #[no_mangle]
        pub static mut ACTIVE_PILLSMOD: Option<&'static dyn PillsMod> = Some(&DRIVER_INSTANCE);
    };
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
| **物理目录** | `mcore/src/pillsmod/` | `userspace/addons/` |
| **编译目标** | `aarch64-unknown-none` | `aarch64-unknown-capsule` |
| **崩溃影响面** | 致命 (引起 Kernel Panic) | 隔离 (仅驱动进程崩溃，服务可热重启) |
| **I/O 传输效率**| 极致 (直接 DMA 映射，零拷贝) | 较高 (依赖内核 MMIO 代理与 Port 唤醒) |
| **中断响应延迟**| **0 延迟** (物理中断向量直达) | **低延迟** (依赖内核上下文调度唤醒) |
| **典型代表设备**| 磁盘 (`virtio_blk`)、网卡 (`virtio_net`) | 显卡 (`gpud`)、触控板 (`touchd`) |
