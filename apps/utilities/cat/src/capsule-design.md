# cat 与 CapsuleOS 深度对接与零拷贝读取设计方案

本篇文档是针对 `cat` 命令行工具在 CapsuleOS 微内核架构上的对接与技术实现规范。为了最大化利用系统的 **Channel 进程间通信通道** 与 **VMO (Virtual Memory Object) 零拷贝机制**，本方案摒弃了传统的多次内核态拷贝设计，提出了一套纯净且高效的数据流动协议。

---

## 目录

1. [CapsuleOS VFS 零拷贝读写本质 (VMO 机制)](#1-capsuleos-vfs-零拷贝读写本质-vmo-机制)
2. [cat 与 fileagent (VFS 代理) 对接交互协议](#2-cat-与-fileagent-vfs-代理-对接交互协议)
3. [零内存碎片、无分配的流式缓冲区设计](#3-零内存碎片无分配的流式缓冲区设计)
4. [移植对接 hnxlibc 精准代码示例](#4-移植对接-hnxlibc-精准代码示例)

---

## 1. CapsuleOS VFS 零拷贝读写本质 (VMO 机制)

在传统的类 Unix 操作系统中，`read` 系统调用要求内核将数据从“物理磁盘缓存（Page Cache）”复制到“内核缓冲区”，再由“内核缓冲区”复制到“用户态缓冲区”，产生了多次 CPU 拷贝与模式切换开销。

CapsuleOS 微内核采用了先进的 **VMO (Virtual Memory Object) 句柄传递** 设计：
1. **统一共享物理页**：内核将物理页面映射为用户态和 VFS 进程都可控的虚拟内存对象（VMO）。
2. **句柄拷贝与共享**：VFS 服务进程（`fileagent`）读取磁盘或内存文件后，将文件的物理页面封装为一个 VMO 句柄，并通过 IPC Channel 将该句柄复制（Duplicate）发送给 `cat` 进程。
3. **极速零拷贝直接寻址**：`cat` 进程获得该 VMO 句柄后，直接通过 `vmo_read` 系统调用在物理内存上寻址并将数据送至其打印流，实现极速读取！

---

## 2. cat 与 fileagent (VFS 代理) 对接交互协议

`cat` 在向 VFS 代理索取文件时，由于其只需要 **“只读（Read-Only）”** 权限，整个 IPC 时序交互图如下：

```
 +---------+               +-------------+               +---------------+
 |   cat   |               |   hnxlibc   |               |  fileagent    |
 +---------+               +-------------+               +---------------+
      |                           |                              |
      |--- 1. open(path) -------->|                              |
      |                           |--- 2. Channel Lookup --------|
      |                           |    ("svc.vfs")               |
      |                           |                              |
      |                           |--- 3. Send OpenCmd --------->|
      |                           |    (Path, Flags=O_RDONLY)    |
      |                           |                              |
      |                           |<-- 4. Return SessionChan ----|
      |                           |    & remote_fd               |
      |<-- 5. Return Local fd ----|                              |
      |                           |                              |
      |--- 6. read(fd, buf) ----->|                              |
      |                           |--- 7. Send ReadCmd --------->|
      |                           |    (remote_fd, len)          |
      |                           |                              |
      |                           |<-- 8. Return result, --------|
      |                           |    VMO Handle (Data inside)  |
      |                           |                              |
      |                           |--- 9. vmo_read(vmo, buf) ----|
      |                           |    (Direct Copy via hardware)|
      |<-- 10. Fill target buf ---|                              |
```

---

## 3. 零内存碎片、无分配的流式缓冲区设计

由于 CapsuleOS 的早期用户态没有常驻的、复杂的动态堆内存分配器，`cat` 工具采用编译期在**物理栈帧**上直接分配的硬核流式吞吐：

```rust
pub fn run_cat<F: FileSystem>(env: &F, path: &str) -> Result<(), FsError> {
    // 静态 2048 字节栈缓存
    let mut buffer = [0u8; 2048];
    let fd = env.open(path)?;

    loop {
        // 通过 FileSystem 接口直接拉取文件数据
        // 底层会复用 hnxlibc 对 VMO 进行物理页共享和直读
        match env.read(fd, &mut buffer) {
            Ok(0) => break, // EOF 正常结束
            Ok(bytes) => {
                env.write_stdout(&buffer[..bytes]);
            }
            Err(e) => {
                env.close(fd);
                return Err(e);
            }
        }
    }
    env.close(fd);
    Ok(())
}
```

---

## 4. 移植对接 hnxlibc 精准代码示例

当 `hnxlibc` 与操作系统 VFS 模块打通后，修改 `cat/src/env/capsule.rs` 文件，将对应的占位存根一并重写对接：

```rust
use super::{FileSystem, FsError};

// 引入外部 Capsule OS 的 libc 库
extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open(&self, path: &str) -> Result<i32, FsError> {
        // 调用 hnxlibc 提供的高层 POSIX O_RDONLY 只读打开
        // 内部会自动建立与 svc.vfs 的 channel 通信并返回客户端虚拟 fd
        let c_path = path.as_ptr(); // 确保以 null 结尾或使用 Rust 高层封装
        let fd = hnxlibc::open(c_path, 0, 0); // 0 代表 O_RDONLY
        if fd >= 0 {
            Ok(fd)
        } else if fd == -2 { // ENOENT
            Err(FsError::FileNotFound)
        } else if fd == -13 { // EACCES
            Err(FsError::PermissionDenied)
        } else {
            Err(FsError::Unknown)
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, FsError> {
        // fd >= 3 进入 hnxlibc 微内核 VMO 零拷贝直读流程
        // fd < 3 进入底层物理串口直接读取
        let res = hnxlibc::read(fd, buf.as_mut_ptr(), buf.len());
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(FsError::Unknown)
        }
    }

    fn close(&self, fd: i32) {
        let _ = hnxlibc::close(fd);
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
```
