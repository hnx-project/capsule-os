# CapsuleOS - 核心系统服务设计与开发规范 (System Services & Bootstrap Specification)

本规范制定了 **CapsuleOS (代号: Pangu)** 在 2.0 "Zero-POSIX Kernel" 时代下，最底层的三大核心用户态特权服务：**`loader` (PID 1)**、**`devmgr` (设备管理服务)** 和 **`fileagent` (虚拟文件系统服务)** 的启动自举序列、模块依赖解耦铁律以及 0-POSIX / 0-VFS 开发标准。

---

## 🧭 1. 三大服务核心职责划分

| 服务名称 (二进制名称) | 进程 ID (PID) | 核心职责与特权 |
| :--- | :--- | :--- |
| **`loader`** (`system/bin/loader`) | **PID 1** | 首个用户态启动进程（自举锚点）。负责从 BootFS VMO 内存中直接解析并加载、派生出其他所有系统守护进程，监控其生命周期（Zombie Reaping / Respawn）。 |
| **`devmgr`** (`system/bin/devmgr`) | **PID 2** | 设备管理器。负责硬件设备树（DTB）解析、平台总线枚举、物理驱动控制（如 UART、中断、网络适配器硬件）以及设备节点发布。 |
| **`fileagent`** (`system/bin/fileagent`) | **PID 3** | VFS 守护进程。接管物理只读/读写文件系统、统一路径解析与会话管理、向应用层提供标准 VFS IPC 访问。 |

---

## ⛓️ 2. 无状态启动自举序列 (Startup Sequence)

整个系统的启动是一个 **“从纯内存能力对象 (VMO) 向有状态 VFS 映射空间”** 演进的交接过程：

```
[  HNX Bootloader  ] -> 物理多段加载 (Kernel, BootFS VMO, DTB)
         |
         v
[  loader (PID 1)  ] -> 1. 纯粹依赖 libcapsule (0-POSIX, 0-VFS, 0-FD)
         |              2. 提取 BootFS VMO, 用户态解析并释放 devmgr 和 fileagent
         v
[  devmgr (PID 2)  ] -> 1. 解析硬件设备树，初始化核心总线和 PL011 驱动
         |              2. 接管中断并启动设备管理器
         v
[ fileagent (PID 3) ] -> 1. 接管 BootFS VMO, 挂载只读根目录
         |              2. 向内核注册 "svc.vfs" 核心通道
         v
[  osh (PID 4) shell ] -> 1. 连接 "svc.vfs", 开启 libc 有状态 FdTable 接管时代
                        2. 开启 100% 完整的标准 POSIX 支持
```

---

## 📐 3. 自举进程开发标准与无状态 `libcapsule` 原语

核心系统服务（`loader`、`devmgr`、`fileagent`）在代码层面上被视作 **“自举层程序 (Bootstrap Layer Programs)”**。它们必须遵循以下极其严格的开发约束：

### 3.1 严禁链接和使用 `libc` 的有状态 I/O
* 自举程序不链接含有 `USER_FD_TABLE` 的 standard `libc` 进行输入输出。
* 代码中绝对不允许出现以下调用：
  * `open()`, `close()`, `read()`, `write()`, `lseek()`, `printf()`, `scanf()`

### 3.2 强依赖无状态的 `libcapsule` 能力原语
自举程序所有的硬件、内存、进程和数据操作，必须显式通过 `libcapsule` 的纯净、无状态系统原语实现：

1. **極早期日志打印**：
   * 严禁调用 `printf`。
   * 自备极简无状态调试打印宏：
     ```rust
     // 内部无 USER_FD_TABLE 查表，直连早期内核 UART 调试写
     pub fn service_log_print(msg: &str) {
         let _ = libcapsule::syscalls::sys_write_debug(msg.as_ptr(), msg.len());
     }
     ```
2. **0-VFS 只读镜像解包 (VMO-Direct Read)**：
   * `loader` 无需打开文件。直接操纵传入的 `BootFS_VMO` 句柄：
     ```rust
     // 直接在用户态从物理内存中解析可执行服务二进制数据
     pub fn load_service_binary(vmo_handle: usize, offset: usize, buf: &mut [u8]) -> Result<usize> {
         libcapsule::vmo_read(vmo_handle, offset, buf)
     }
     ```
3. **特权进程能力派生**：
   * `loader` 使用纯能力原语创造空白进程并映射 OHLINK 的 TEXT/DATA 等镜像：
     ```rust
     let child_proc = libcapsule::process_create("hnx-devmgr")?;
     libcapsule::load_binary(child_proc, service_vmo)?;
     libcapsule::process_start(child_proc)?;
     ```

---

## 🔑 4. `libcapsule` 针对自举服务（Service Program）的无状态封装包设计

由于自举层程序需要绝对脱离 `libstd` 的有状态包装，`libcapsule` 除了底层的裸系统调用宏外，还必须封装一套面向自举服务的 **RAII 化、无状态能力辅助库**：

### 4.1 极早期无状态控制台格式化打印器 (`libcapsule::kprintln!`)
核心自举服务需要最起码的日志打印，`libcapsule` 将提供一套实现 `core::fmt::Write` 的极简同步打印机：
```rust
use core::fmt::{self, Write};

struct DebugWriter;

impl Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            // 直接触发内核 Debug 写系统调用 (SYSCALL_WRITE to fd=1)，跳过任何 FdTable
            let _ = syscall!(
                shared::syscall_nums::SYSCALL_WRITE,
                1, s.as_ptr() as usize, s.len(), 0, 0, 0
            );
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        let mut writer = DebugWriter;
        let _ = core::fmt::write(&mut writer, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => {{
        $crate::kprint!($($arg)*);
        $crate::kprint!("\n");
    }};
}
```

### 4.2 RAII 化的纯能力对象指针 (`libcapsule::Channel`)
在没有 POSIX FD 表的情况下，用户态通道资源管理完全转为 RAII 生命期托管：
```rust
pub struct Channel {
    handle: usize,
}

impl Channel {
    pub fn from_raw(handle: usize) -> Self {
        Self { handle }
    }

    pub fn write(&self, data: &[u8], handles: &[u32]) -> Result<usize, Status> {
        crate::syscalls::channel_write(self.handle, data, handles)
    }

    pub fn read(&self, buf: &mut [u8], handles: &mut [u32]) -> Result<usize, Status> {
        crate::syscalls::channel_read(self.handle, buf, handles)
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // 当对象生命周期结束时，自动物理关闭内核句柄，防止能力泄露
        let _ = crate::syscalls::close(self.handle);
    }
}
```

### 4.3 0-FD 程序解包加载器 (`libcapsule::ServiceLoader`)
在 `libcapsule` 中提供基于 `VMO` 的用户态解包和程序加载器，将 OHLINK 的物理映射逻辑与 VFS 文件路径彻底切断：
```rust
pub struct ServiceLoader {
    bootfs_vmo: usize,
}

impl ServiceLoader {
    pub fn new(bootfs_vmo: usize) -> Self {
        Self { bootfs_vmo }
    }

    /// 从 BootFS VMO 的特定偏移量解析并拉起一个服务程序
    pub fn spawn_service(
        &self, 
        name: &str, 
        offset: usize, 
        size: usize
    ) -> Result<usize, Status> {
        let proc_handle = crate::syscalls::process_create(name)?;
        
        // 1. 创建该服务的只读子 VMO 对象
        let service_vmo = crate::syscalls::vmo_create_child(self.bootfs_vmo, offset, size)?;

        // 2. 内核载入子 VMO 并直接解析、建立 VMAR 镜像段
        crate::syscalls::load_binary(service_vmo, name)?;

        // 3. 派生其主线程，完成 EL0 拉起
        // ... (thread_create / thread_start)
        Ok(proc_handle)
    }
}
```

---

## 🔒 5. 0-POSIX / 0-VFS 安全边界

在 CapsuleOS 2.0 中，服务层拥有极强的安全隔离和自愈优势：

1. **崩溃不级联 (No Cascade Failure)**：
   * 即使 `fileagent` 或 `devmgr` 因为磁盘坏道、驱动异常而发生崩溃（EL0 Panic），由于它们不运行在内核态（EL1），内核完全不会挂起。
2. **热重启 (Hot Respawn)**：
   * 作为常驻自举锚点的 `loader` (PID 1) 在通过 `libcapsule::wait4` 捕获到 `fileagent` 或 `devmgr` 的退出事件时，可以直接重新读取 `BootFS_VMO`，极速、就地拉起一个全新的服务替代版，实现高可用微内核的高自愈特性！
3. **能力审计 (Capability Audit)**：
   * 普通应用只有向 `loader` 或 `devmgr` 连接并请求到特定的句柄才能访问特定硬件。内核 `HandleTable` 的权限过滤器使得非法进程完全无法绕过用户态服务去篡改硬件寄存器，消除了传统单体内核的“全特权提权”漏洞。

---
*本文档由 CapsuleOS 开源工程 AI 辅助架构师 opencode 与 HNX-Project 管理组共同制定。*
