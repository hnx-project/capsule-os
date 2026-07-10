# touch 与 CapsuleOS 对接设计规范

为了在 CapsuleOS 完成用户态环境后能够创建空白文件或更新元数据时间戳，本篇文档规范了 `touch` 命令行工具的对接方案。

---

## 1. 机制说明

在 CapsuleOS 中，创建一个空文件需要通过 `open` 系统调用并传入 `O_CREAT` (创建) 标志。
由于 `hnxlibc` 原生支持了 `open` 和 `close`：
1. `touch` 工具只需传入 `O_CREAT | O_WRONLY` (对应常数值，例如 `0x40 | 0x1`，需在 CapsuleOS 常数定义中核对) 打开指定路径文件。
2. 获得合法的虚拟文件描述符 `fd` 后，无需写入数据，直接调用 `close(fd)` 关闭，即完成了空文件的物理创建。

---

## 2. 移植对接 `hnxlibc` 的最终实现

重写 `touch/src/env/capsule.rs` 即可打通对接：

```rust
use super::{FileSystem, FsError};

// 引入 Capsule OS 用户态基础 libc 库
extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn touch(&self, path: &str) -> Result<(), FsError> {
        let c_path = path.as_ptr(); // 确保以 null 结尾
        // 传入 O_CREAT | O_WRONLY，假设常数值为 0x41 (具体需与系统 VFS 头文件常数对齐)
        let fd = hnxlibc::open(c_path, 0x41, 0o644);
        if fd >= 0 {
            let _ = hnxlibc::close(fd);
            Ok(())
        } else if fd == -2 { // ENOENT
            Err(FsError::PathNotFound)
        } else if fd == -13 { // EACCES
            Err(FsError::PermissionDenied)
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
