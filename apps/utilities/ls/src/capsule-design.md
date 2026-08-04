# ls 与 CapsuleOS 目录迭代与对接规范设计方案

本篇文档详述了 `ls` (Directory Content Lister) 在 CapsuleOS 上的移植规划。为了保证在微内核、无标准库且没有频繁动态内存分配的极致场景下安全、平滑地输出目录拓扑，本设计方案规划了 VFS 通道底层的 **`ReadDir` 扩展协议** 与高效直观的 **`Dirent` 对齐页存储**。

---

## 目录

1. [微内核 VFS 目录浏览本质](#1-微内核-vfs-目录浏览本质)
2. [ReadDir 跨进程 IPC 对接协议](#2-readdir-跨进程-ipc-对接协议)
3. [128 字节物理对齐的 C-Dirent 结构](#3-128-字节物理对齐的-c-dirent-结构)
4. [移植对接 hnxlibc 精准代码示例](#4-移植对接-hnxlibc-精准代码示例)

---

## 1. 微内核 VFS 目录浏览本质

在 CapsuleOS 中，所有的物理/虚拟文件系统都由独立的服务进程 `fileagent` (`svc.vfs`) 代理控制。由于 `readdir` (读取目录项) 涉及多个未知长度的文件名字串，为了避免传统的字符串高开销传递与内存碎片：
1. **平坦 C 结构体流式传输**：目录项在跨进程传递时被格式化为完全对齐、平坦分布的字节缓存（`Dirent`），不使用任何指针嵌套。
2. **一次性单项流式索取**：`ls` 客户端通过单向 channel 每次索取一个目录项，直至读完，极大减轻了早期内核和 `fileagent` 的通信总线带宽压力。

---

## 2. ReadDir 跨进程 IPC 对接协议

`ls` 会话在通过 `open` 拿到一个代表目录的虚拟文件描述符 `fd` 后，其向 `svc.vfs` 迭代索取物理页的核心时序如下：

1. **VFS 命令层扩充**：在 `hnxlibc` 的 `FileAgentCmd` 枚举中需要扩充 `ReadDir` 信号：
   ```rust
   enum FileAgentCmd {
       // ... 现有 Open, Read ...
       ReadDir {
           fd: u32,
       }
   }
   ```
2. **时序状态循环**：
   * `ls` 进程通过 TTY 标准输出打印，底层向 `svc.vfs` 会话信道发起 `ReadDir { fd }` 请求。
   * 服务端收到请求后，从当前的目录流偏移（Offset）中读取下一个有效条目，格式化并回填写入到共享的 VMO，或者通过对偶通道数据包直复回传给客户端，最后将服务端偏移加一。
   * 当读取到末尾，服务端在状态回执中返回 `0` (EOF)。

---

## 3. 128 字节物理对齐的 C-Dirent 结构

在 `ls/src/env/mod.rs` 中，我们定义了以下契合硬件总线对齐要求的定长轻量目录项结构体：

```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent {
    pub ino: u64,        // 8 字节：Inode 索引节点号
    pub ftype: u8,       // 1 字节：文件类型 (0 = 未知, 1 = 普通文件, 2 = 目录)
    pub name_len: u8,    // 1 字节：文件名真实长度
    pub name: [u8; 118], // 118 字节：定长文件名缓存
                         // 结构体总计：8 + 1 + 1 + 118 = 128 字节 (100% 缓存行物理对齐)
}
```

---

## 4. 移植对接 hnxlibc 精准代码示例

当 CapsuleOS 上的 VFS 驱动完成扩充后，您可以随时重写 `ls/src/env/capsule.rs` 文件，以下是高保真对接示例：

```rust
use super::{Dirent, FileSystem, FsError};

// 引入外部 Capsule OS 的 libc 库
extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open_dir(&self, path: &str) -> Result<i32, FsError> {
        // 调用 hnxlibc 提供的 O_DIRECTORY 模式打开，获取文件系统服务通道映射
        let c_path = path.as_ptr();
        let fd = hnxlibc::open(c_path, 0x10000, 0); // 假设 0x10000 代表 O_DIRECTORY
        if fd >= 0 {
            Ok(fd)
        } else if fd == -2 {
            Err(FsError::DirectoryNotFound)
        } else {
            Err(FsError::Unknown)
        }
    }

    fn readdir(&self, fd: i32, dirent: &mut Dirent) -> Result<bool, FsError> {
        // 将 &mut Dirent 的引用转为原始指针，交由 hnxlibc 底层 IPC 通道去填充
        let ptr = dirent as *mut Dirent as *mut u8;
        let res = hnxlibc::readdir(fd, ptr);
        if res > 0 {
            Ok(true) // 成功读取一条
        } else if res == 0 {
            Ok(false) // 读取完毕 (EOF)
        } else {
            Err(FsError::Unknown)
        }
    }

    fn close_dir(&self, fd: i32) {
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
