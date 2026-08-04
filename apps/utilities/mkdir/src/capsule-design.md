# mkdir 与 CapsuleOS 对接设计规范

为了在 CapsuleOS 完成用户态环境后能够创建目录，本篇文档规范了 `mkdir` 命令行工具的对接方案。

---

## 1. 机制说明

在 CapsuleOS 中，创建目录（mkdir）是一个直接改变虚拟文件系统元数据（Metadata）的写操作。
由于已有的 `hnxlibc` 已经内置并导出了高层 POSIX-like 的 `pub extern "C" fn mkdir(path: *const u8) -> i32` 接口，它内部会自动向 `svc.vfs` 投递 `MkDir` 指令包，因此我们在用户态可以非常自然地完成移植。

---

## 2. 移植对接 `hnxlibc` 的最终实现

重写 `mkdir/src/env/capsule.rs` 即可打通对接：

```rust
use super::{FileSystem, FsError};

// 引入 Capsule OS 用户态基础 libc 库
extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn mkdir(&self, path: &str) -> Result<(), FsError> {
        let c_path = path.as_ptr(); // 确保以 null 结尾，或者 hnxlibc 的 mkdir 已做处理
        let res = hnxlibc::mkdir(c_path);
        if res == 0 {
            Ok(())
        } else if res == -17 { // EEXIST (AlreadyExists)
            Err(FsError::AlreadyExists)
        } else if res == -2 {  // ENOENT (PathNotFound)
            Err(FsError::PathNotFound)
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
