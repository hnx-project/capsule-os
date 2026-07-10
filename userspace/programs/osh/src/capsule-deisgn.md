# osh 与 CapsuleOS 对接与移植规范文档

为了在 CapsuleOS 完成用户态环境后，能够无缝将 `osh` (Capsule OS Shell) 移植并作为系统的首选交互 shell，本篇文档详细说明了当前在 `no_std` 下 `osh` 缺失的底层支撑部件、要实现的系统调用，以及如何优雅地对接已有的 `hnxlibc` 库。

---

## 目录

1. [当前架构总览 (PAL)](#1-当前架构总览-pal)
2. [hnxlibc 现状深度剖析](#2-hnxlibc-现状深度剖析)
3. [缺失的底层支撑及补齐方案（CapsuleOS 需要实现什么）](#3-缺失的底层支撑及补齐方案capsuleos-需要实现什么)
4. [对接 hnxlibc 的最佳实现细节](#4-对接-hnxlibc-的最佳实现细节)
5. [重定向与管道的 Zsh 级微内核进化方案（高阶设计）](#5-重定向与管道的-zsh-级微内核进化方案高阶设计)
6. [交叉编译与根文件系统打包集成](#6-交叉编译与根文件系统打包集成)

---

## 1. 当前架构总览 (PAL)

`osh` 采用平台抽象层 **PAL** (Platform Abstraction Layer) 设计。所有涉及到 I/O、文件系统及进程环境的操作，均被定义在 `src/env/mod.rs` 的 `Environment` 特征（Trait）中。

在 `no_std` (开启 `capsule` 特性) 编译时，对应的实现定义在 `src/env/capsule.rs` 的 `CapsuleEnv` 结构体中。目前的函数均是空存根（Stub），仅为了在没有 `std` 时通过基础编译。

---

## 2. hnxlibc 现状深度剖析

通过阅读 `capsule-os/userspace/hnxlibc` 源码，该用户态库采用了一种非常现代化且高效的微内核架构（类似于 Fuchsia OS）：
* **虚拟文件系统 (VFS)**：核心功能委派给 `svc.vfs` 服务进程（`fileagent`）。
* **跨进程 IPC 通道**：利用内核的 `SYSCALL_CHANNEL_READ` / `SYSCALL_CHANNEL_WRITE` 实现会话消息推送。
* **零拷贝文件读写**：采用 `SYSCALL_VMO_CREATE`、`SYSCALL_VMO_READ`、`SYSCALL_VMO_WRITE` 临时创建虚拟内存对象 (VMO) 句柄进行进程间的数据零拷贝交换。
* **进程替换接口**：自带 `SYSCALL_EXEC` (对应 `exec_impl(name: &str) -> i32`) 支撑外部程序拉起。

---

## 3. 缺失的底层支撑及补齐方案（CapsuleOS 需要实现什么）

目前要使 `osh` 完全就绪，仅需要补齐以下两个主要部分：

### A. 获取/改变当前工作目录 (核心缺失)
* **现状**：`hnxlibc` 目前提供了 `open`、`read`、`write`、`mkdir`、`rmdir` 等常规 POSIX-like 接口，但未提供 `getcwd` 和 `chdir` 的系统调用或 VFS 服务通道指令。
* **内核/VFS 补齐方案**：
  1. 在 `FileAgentCmd` 中新增两个成员：
     ```rust
     enum FileAgentCmd {
         // ... 现有 Open, Read ...
         GetCwd,
         ChDir {
             path: [u8; 128],
             path_len: u32,
         }
     }
     ```
  2. 内核在进程的 PCB (Process Control Block) 中存储和维护该进程的当前工作目录（CWD）。

### B. TTY/UART 阻塞式单字符读取
* **现状**：`hnxlibc` 的 `getchar()` 内部直接通过 `read(0, buf, 1)` 从标准输入通道拉取。
* **要求**：为了确保在 REPL 交互式运行模式下不发生死循环或高频空轮询，底层的物理串口或终端读取系统调用应当在没有键入输入时进入阻塞状态（Blocking），直至用户键入字符。

---

## 4. 对接 hnxlibc 的最佳实现细节

当 CapsuleOS 补齐了 `getcwd` 和 `chdir` 之后，将 `osh/src/env/capsule.rs` 完全重写为以下与 `hnxlibc` 深度对接的代码：

```rust
use super::{Environment, ShellError};

// 强制引入外部 Capsule OS 的 libc 库
extern crate hnxlibc;

pub struct CapsuleEnv;

impl Environment for CapsuleEnv {
    fn write_stdout(&self, data: &[u8]) {
        // fd = 1 为标准输出。在 hnxlibc 中会直接回退调用内核串口 write 系统调用
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        // fd = 2 为标准错误
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        // fd = 0 为标准输入
        let res = hnxlibc::read(0, buf.as_mut_ptr(), buf.len());
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(ShellError::IoError)
        }
    }

    fn getcwd(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        // 等待 hnxlibc 提供封装后的对接
        let res = hnxlibc::getcwd(buf.as_mut_ptr(), buf.len());
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(ShellError::Unknown)
        }
    }

    fn chdir(&self, path: &str) -> Result<(), ShellError> {
        // 传递给 hnxlibc 的 chdir 接口
        let res = hnxlibc::chdir(path);
        if res == 0 {
            Ok(())
        } else if res == -2 { // 假定 -2 为 ENOENT (PathNotFound)
            Err(ShellError::PathNotFound)
        } else {
            Err(ShellError::Unknown)
        }
    }

    fn get_env(&self, key: &str, buf: &mut [u8]) -> Result<usize, ShellError> {
        // 对接 hnxlibc 的 getenv 接口
        let res = hnxlibc::getenv(key, buf.as_mut_ptr(), buf.len());
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(ShellError::PathNotFound)
        }
    }

    fn set_env(&self, key: &str, value: &str) -> Result<(), ShellError> {
        // 对接 hnxlibc 的 setenv 接口
        let res = hnxlibc::setenv(key, value);
        if res == 0 {
            Ok(())
        } else {
            Err(ShellError::Unknown)
        }
    }

    fn print_envs(&self) {
        // 调用 hnxlibc 的环境变量打印调试接口
        hnxlibc::print_envs();
    }

    fn execute(&self, cmd: &str, args: &[&str]) -> Result<i32, ShellError> {
        // 传递命令名与参数给 hnxlibc 的 exec 系统调用封装进行进程加载与替换
        let code = hnxlibc::exec(cmd);
        Ok(code)
    }

    fn read_file(&self, path: &str, buf: &mut [u8]) -> Result<usize, ShellError> {
        let fd = hnxlibc::open(path.as_ptr(), 0, 0); // O_RDONLY
        if fd >= 0 {
            let res = hnxlibc::read(fd, buf.as_mut_ptr(), buf.len());
            let _ = hnxlibc::close(fd);
            if res >= 0 {
                return Ok(res as usize);
            }
        }
        Err(ShellError::IoError)
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
```

---

## 5. 重定向与管道的 Zsh 级微内核进化方案（高阶设计）

在自研 Zsh 体验阶段，我们可以利用 CapsuleOS 微内核已就绪的高阶原语进行极客设计，绕过传统 Unix 繁重的 `dup2` 机制：

### A. 应用级重定向 (Redirect)
在解析到 `cmd > filename` 后，由于 `hnxlibc` 已具备功能完备的 `open` 协议，`osh` 无需内核支持 `dup2`。
* **实现**：`osh` 内部在执行命令时：
  1. 调用 `hnxlibc::open(filename)` 获得虚拟 `fd`。
  2. 在将该命令作为外部程序启动前，将虚拟 `fd` 绑定或利用 VMO 句柄传给外部进程作为其 standard output，直接完成重定向。

### B. 基于 Channel 的高性能管道 (Pipeline)
不需要在内核层实现传统的 Unix `pipe` 驱动，可以直接在用户态利用 `hnxlibc::channel_create()` 实现：
* **工作原理**：
  1. 当 `osh` 遇到 `cmd1 | cmd2` 时，直接通过用户态调用 `channel_create` 建立一对端点。
  2. 在拉起 `cmd1` 时，将写入端通道句柄随启动参数（或特定句柄插槽）传递给 `cmd1`；拉起 `cmd2` 时传递读取端。
  3. 这种基于微内核消息通道的管道设计极其高效，甚至支持超大批量数据的 VMO 零拷贝级共享传输，性能远超传统 Linux。

---

## 6. 交叉编译与根文件系统打包集成

当内核与 libc 编译就绪，在 `capsule-os` 的 `xtask` 或主构建流水线中，可以如此交叉编译和打入根文件系统：

1. **编译 `osh` 二进制**：
   ```bash
   # 进入 programs/osh 目录
   # 针对 CapsuleOS 对应架构（以 riscv64 目标平台为例）进行交叉编译
   # 必须关闭默认的 host 特性，并激活 capsule 特性
    ```

2. **打入 staging_rootfs 镜像**：
   将其复制到 `capsule-os` 的根文件系统 staging 目录：

3. **设为系统默认 Shell**：
   在 `capsule-os` 的 `init` 进程（如 `userspace/services/init/src/main.rs`）中，在初始化设备和文件系统完毕后，调用 `sys_execve("/system/bin/osh", ...)` 即可直接将控制台交给 `osh`，开启优雅的交互式系统体验！
