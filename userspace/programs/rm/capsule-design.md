# rm / rmdir 与 CapsuleOS 对接与移植规范文档

为了在 CapsuleOS 文件系统完善后实现用户态物理文件及目录的删除，本篇文档对 `rm` 与 `rmdir` 工具有机关联与底层对接流程进行了规范定义。

---

## 1. 系统机制映射

在 CapsuleOS 微内核架构下，删除一个文件节点（`rm`）或删除一个空白目录项（`rmdir`）均是由用户态向系统的虚拟文件服务通道（`svc.vfs`）发送对应的状态改变请求实现的。
* **`rm` (文件删除)**：底层对接 `hnxlibc::unlink`，对应内核 `SYSCALL_CLOSE` 后端或专门的文件去激活指令，通知 `fileagent` 释放相应 VMO 节点并清空 FAT/RamFS 物理扇区。
* **`rmdir` (目录删除)**：对接 `hnxlibc::rmdir`，通知 VFS 服务将对应子目录元数据节点从父节点树上剪除，且如果目录非空（返回 `ENOTEMPTY`），则阻断删除操作，确保安全。

---

## 2. 移植对接 `hnxlibc` 的最终实现

当 CapsuleOS 上的这组删除动作封装于 `hnxlibc` 就绪后，替换对应项目的 `src/env/capsule.rs` 即可实现快速端到端闭环。

### A. `rm` 对接代码
```rust
use super::{FileSystem, FsError};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn unlink(&self, path: &str) -> Result<(), FsError> {
        let c_path = path.as_ptr();
        let res = hnxlibc::unlink(c_path);
        if res == 0 {
            Ok(())
        } else if res == -2 { // ENOENT
            Err(FsError::FileNotFound)
        } else {
            Err(FsError::Unknown)
        }
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

### B. `rmdir` 对接代码
```rust
use super::{FileSystem, FsError};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn rmdir(&self, path: &str) -> Result<(), FsError> {
        let c_path = path.as_ptr();
        let res = hnxlibc::rmdir(c_path);
        if res == 0 {
            Ok(())
        } else if res == -2 { // ENOENT
            Err(FsError::DirectoryNotFound)
        } else if res == -39 { // ENOTEMPTY (假定 39 为 POSIX 标准号)
            Err(FsError::NotEmpty)
        } else {
            Err(FsError::Unknown)
        }
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
