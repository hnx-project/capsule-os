# CapsuleOS - POSIX 2.0 终极计划：内核零 POSIX 认知 (Zero-POSIX Kernel Plan)

本规划书确立了 **CapsuleOS (代号: Pangu)** 的终极标准兼容架构：**彻底取消内核中的任何 POSIX 认知与状态维护，将所有的 POSIX 虚拟 FD、状态机及 API 转发逻辑完全剥离，全盘下放到用户态 `libraries/libc` 中通过对 `libcapsule` 对象能力原语的组合模拟来实现。**

这使得 CapsuleOS 内核在 2.0 时代蜕变为一个极其纯粹、安全、高内聚的 “内核零 POSIX 认知 (Zero-POSIX Kernel)” 的现代微内核。

---

## 🌌 1. 终极微内核主义设计哲学

在传统单体内核中，内核需要为每个进程维护庞大的文件描述符表、信号状态、管道以及网络套接字等一系列复杂的 POSIX 状态机，这导致了单体内核庞大、臃肿且极易因为某个模块崩溃导致整机瘫痪。

在 CapsuleOS 2.0 的 Zero-POSIX Kernel 哲学下：
1. **内核零感知**：内核不知道什么是文件、什么是 `fd`（文件描述符）、更不知道什么是网络套接字或管道。内核只提供纯粹、无状态的对象能力（Capability）原语：`VMO`、`VMAR`、`Channel`、`Port` 等。
2. **用户态托管**：应用层调用的 `open`、`read`、`write` 等标准 C API，完全由用户态的 `libraries/libc` 通过查表与微内核 IPC 转发在用户空间进行状态托管。
3. **安全与模块解耦**：即便负责管理虚拟文件系统的 `fileagent` 或者是管理网络套接字的 `netstack` 崩溃，由于其运行在隔离的用户态 EL0 沙箱中，也不会损害内核本身及其他任何进程的安全，系统可以优雅地对其进行独立热重启。

---

## 🔌 2. 用户态虚拟描述符表 (User-Space FdTable) 机制

在 `libraries/libc` 中，我们将自行管理进程专属的局部虚拟描述符表，将传统的 `int fd` 映射为对应的微内核通道或其他原生对象。

### 2.1 描述符类型与数据结构
```rust
pub enum FdType {
    /// 虚拟控制台 (对应 fd=0, 1, 2)
    /// 直接通过 Channel 句柄连接到控制台或调试输出端
    Console {
        channel_handle: usize,
    },
    /// VFS 真实文件
    /// channel_handle 指向通往 VFS (fileagent) 的文件独享会话通道
    File {
        channel_handle: usize,
        remote_fd: u32,
    },
    /// 管道
    /// channel_handle 为本管道端点所绑定的通道句柄
    Pipe {
        channel_handle: usize,
        is_write: bool,
    },
    /// 套接字
    /// channel_handle 指向通往 netstack 协议栈 of 传输通道
    Socket {
        channel_handle: usize,
    }
}

pub struct FdEntry {
    pub flags: u32,
    pub r#type: FdType,
}

// libc 内部维护 of 全局 FdTable 静态数组 (默认预填 0, 1, 2 指向控制台)
pub static mut USER_FD_TABLE: [Option<FdEntry>; 64] = ...;
```

### 2.2 用户态经典 API 转发映射流程

1. **`open(path, flags)`**：
   * `libc` 内部通过 `libcapsule::channel_lookup("svc.vfs")` 连接 VFS。
   * 向 VFS 发送 `FileAgentCmd::Open` 请求。
   * 接收 VFS 的回复，取得专属于该文件的服务会话通道句柄 `file_chan_handle`（这是一个纯粹的微内核 `Channel`，不对应任何 FD）。
   * `libc` 自行在 `USER_FD_TABLE` 寻找空闲插槽（如 `3`），填入并绑定。
   * 向调用者返回 `3`。不产生任何 POSIX 内核调用。

2. **`write(fd, buf, len)`**：
   * `libc` 内部查询 `USER_FD_TABLE[fd]`
   * 若指向控制台，直接向控制台通道发送写请求（或退化为微内核极早期的系统调试输出端口）。
   * 若指向文件或管道，拿到绑定的 `channel_handle`，直接执行 `libcapsule::channel_write(channel_handle, buf, ...)` 塞出。

---

## 📊 3. 终极调用对照关系：应用 POSIX $\to$ `libcapsule` $\to$ 内核 Capability $\to$ 底层实现

在 CapsuleOS 2.0 中，**`libc` 彻底蜕化为 `libcapsule` 核心对象原语的“模拟包装与转发层”**。内核中不存在任何意义上的 `SYSCALL_OPEN`、`SYSCALL_CLOSE` 等 POSIX 系统调用号。

以下是整个操作系统在 2.0 时代最具技术张力和自洽美学的核心调用对照链路：

| 应用层 POSIX 标准接口 | 用户态模拟实现机制 (`libc`) | 调用的 `libcapsule` 接口 | 陷入内核的 Capability 系统调用号 | 内核底层处理文件 |
| :--- | :--- | :--- | :--- | :--- |
| **`open`** | 1. 查找 VFS 服务注册通道。<br>2. 组装 Open 协议消息包并通过通道发送，阻塞等待回复。<br>3. 在本地 `USER_FD_TABLE` 绑定回复回来的专属会话句柄。 | `channel_lookup("svc.vfs")`<br>`channel_write` / `channel_read` | `SYSCALL_CHANNEL_LOOKUP`<br>`SYSCALL_CHANNEL_WRITE` / `_READ` | `kernel/src/syscall/capability/mod.rs`<br>`kernel/src/ipc/channel.rs` |
| **`close`** | 1. 查表获取绑定的 `channel_handle`。<br>2. 向 VFS 投递 Close 协议消息。<br>3. 清空 `USER_FD_TABLE` 本地插槽，物理关闭本地通道。 | `channel_write`<br>`close(channel_handle)` | `SYSCALL_CHANNEL_WRITE`<br>`SYSCALL_CLOSE` | `kernel/src/syscall/capability/mod.rs`<br>`kernel/src/ipc/channel.rs` |
| **`read`** | 1. 查表获取绑定的 `channel_handle`。<br>2. 发送 Read 协议，阻塞接收回复（含临时 VMO 数据句柄）。<br>3. 调用 VMO 读原语拷贝至用户态，释放临时 VMO 句柄。 | `channel_write` / `channel_read`<br>`vmo_read`<br>`close(vmo_handle)` | `SYSCALL_CHANNEL_WRITE` / `_READ`<br>`SYSCALL_VMO_READ`<br>`SYSCALL_CLOSE` | `kernel/src/syscall/capability/mod.rs`<br>`kernel/src/mm/vmo.rs` |
| **`write`** | 1. 查表获取绑定的 `channel_handle`。<br>2. 本地创建临时 VMO，填入数据。<br>3. 伴随 VMO 句柄发送 Write 协议包塞出给 VFS 服务。<br>4. 关闭本地临时 VMO 句柄。 | `vmo_create`<br>`vmo_write`<br>`channel_write`<br>`close(vmo_handle)` | `SYSCALL_VMO_CREATE`<br>`SYSCALL_VMO_WRITE`<br>`SYSCALL_CHANNEL_WRITE`<br>`SYSCALL_CLOSE` | `kernel/src/syscall/capability/mod.rs`<br>`kernel/src/mm/vmo.rs` |
| **`pipe`** | 1. 物理创建一对双端互通通道。<br>2. 本地 `USER_FD_TABLE` 分配两个空闲槽绑定此双端，标明读写向。 | `channel_create` | `SYSCALL_CHANNEL_CREATE` | `kernel/src/syscall/capability/mod.rs`<br>`kernel/src/ipc/channel.rs` |
| **`dup2`** | 1. 获取 `oldfd` 绑定的通道句柄。<br>2. 复制内核中对应的句柄能力引用。<br>3. 在本地 `USER_FD_TABLE` 中将 `newfd` 绑定到新复制出来的句柄。 | `handle_duplicate` | `SYSCALL_HANDLE_DUPLICATE` | `kernel/src/syscall/capability/mod.rs`<br>`kernel/src/object/handle_table.rs` |
| **`fork`** | 1. 触发特权级进程分裂。<br>2. 内核复制进程上下文与 `HandleTable`（通道句柄被自动 duplicate 共享）。<br>3. 子进程继承父进程的 `USER_FD_TABLE`，实现对 VFS 相同文件的 offset 共享。 | `process_create` / `spawn` (进程调控) | `SYSCALL_PROCESS_CREATE` / `SYSCALL_SPAWN` | `kernel/src/syscall/lifecycle/mod.rs`<br>`kernel/src/task/process.rs` |
| **`execve`** | 1. 触发特权级程序重载控制器。<br>2. 内核清理原虚拟地址空间，重映射 OHLINK 并重设寄存器。 | `spawn` / `execve_impl` | `SYSCALL_SPAWN` / `SYSCALL_EXECVE` | `kernel/src/syscall/lifecycle/mod.rs`<br>`kernel/src/loader.rs` |
| **`wait4`** | 向内核生命周期模块发起对指定/任何子进程 PCB 状态的收割与轮询。 | `wait4` | `SYSCALL_WAIT4` | `kernel/src/syscall/lifecycle/mod.rs`<br>`kernel/src/task/process.rs` |

---

## ⛓️ 4. Bootstrap (系统自举) 0-FD 破局机制

为了解决 **“必须先有 fileagent 才能进行任何 VFS/FD 操作，但 fileagent 和 devmgr 自身启动及 loader 读取镜像又严重依赖文件读取”** 的系统自举死锁，CapsuleOS 2.0 采用了极具微内核科学美学、契合 QEMU 物理分段加载的 **“0-FD 内存自举”** 方案。

### 4.1 QEMU 物理分段引导格局
在系统上电时，引导加载程序 (Bootloader) 或 QEMU 会将各个固件与文件系统作为独立的 `force-raw` 段分开加载到不同的物理内存起始地址：
* `bootloader` $\to$ `{boot_addr}` (`0x44000000`)
* `kernel` (hnxcore) $\to$ `{ohc_addr}` (`0x40700000`)
* `rootfs.img` (系统服务归档) $\to$ `{rootfs_addr}` (`0x46000000` 独立的物理内存段)
* `DTB` (Device Tree Blob) $\to$ `{dtb_addr}` (`0x42000000`)

### 4.2 0-FD 内存自举控制流 (Bootstrap Sequence)

1. **内核 VMO 化物理 rootfs.img**：
   内核启动时，通过解析 **DTB** 获取只读文件系统镜像在物理内存中的段范围（`PA = 0x46000000`）。内核利用物理页映射，将其直接包装为一个一等公民的内核对象 —— **`BootFS VMO`**。
2. **启动时句柄分发 (Handles at Startup)**：
   内核拉起首个用户态引导进程 `loader` (PID 1)，并在其 `HandleTable` 中安全地注入指向该 `BootFS VMO` 的只读 `HandleValue` 凭证（作为首要启动能力）。
3. **`loader` 用户态内存直接解包**：
   * `loader` 进程**不使用、也严禁链接**具有有状态 FD/VFS 状态机的 `libc`！
   * `loader` 的运行时直接提取 `BootFS_VMO` 句柄，调用 `libcapsule::vmo_read(bootfs_vmo, offset, buf)` 能力原语。
   * `loader` 在用户态执行无状态的只读解包算法，从 VMO 的指定位置内存段中直接解析并读出 `devmgr` 和 `fileagent` 的 OHLINK 二进制。
4. **拉起服务与动态转让 (Dynamic Handover)**：
   * `loader` 使用 `libcapsule::process_create` 直接拉起 `devmgr` 和 `fileagent` 进程。
   * 同时，将 `BootFS_VMO` 的句柄复制并通过 IPC Channel 完美转让给 `fileagent` 进程，由其进行接管。
5. **VFS 就绪与 POSIX 接管**：
   `fileagent` 初始化完毕，向内核服务注册表注册 `svc.vfs`。自此，后续运行的 EL0 普通应用调用 `open` 时，`libc` 即可成功通过通道建立 VFS 会话，自举死锁完美闭合。

---

## 📐 5. 核心自举服务的隔离铁律 (System Service Isolation Rules)

微内核最底层的系统服务（`loader`、`devmgr`、`fileagent`）必须遵循绝对的开发铁律，从而在工程上彻底斩断 Bootstrap 依赖循环：

* **自举服务铁律一：严禁使用任何有状态 POSIX I/O**
  * `loader`、`devmgr`、`fileagent` 绝对不允许调用 `open`、`close`、`read`、`write` 标准 C 函数，甚至在源码中根本不应包含对带有 `USER_FD_TABLE` 状态的标准 `libc` I/O 接口的链接。
* **自举服务铁律二：强依赖无状态能力原语 `libcapsule`**
  * 所有的 I/O 操作、内存交互、消息投递，必须直接、单一地调用 `libcapsule` 导出的原生 Capability 接口（如 `vmo_read`、`vmo_write`、`channel_write`）。
* **自举服务铁律三：零 VFS 调试打印机制**
  * 核心服务严禁调用 `printf`。其调试日志输出通过 `libcapsule` 直接向内核极早期 Debug 端口发起直连写入（即 `SYSCALL_WRITE` to fd=1/2），完美实现 **"0-POSIX 0-VFS 0-FD"** 的轻量、安全、独立引导。

---
*本文档由 CapsuleOS 开源工程 AI 辅助架构师 opencode 与 HNX-Project 管理组共同制定。*
