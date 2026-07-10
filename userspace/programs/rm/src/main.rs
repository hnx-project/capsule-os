#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::{FileSystem, FsError};

/// 递归删除文件或目录的核心状态机 (no_std 编译安全)
pub fn remove_recursive<F: FileSystem>(env: &F, path: &str, force: bool) -> Result<(), FsError> {
    if env.is_dir(path) {
        // 递归读取目录下所有的子项绝对路径并逐一删除
        let mut err: Option<FsError> = None;
        let _ = env.read_dir_names(path, |sub_path| {
            if err.is_none() {
                if let Err(e) = remove_recursive(env, sub_path, force) {
                    err = Some(e);
                }
            }
        });

        if let Some(e) = err {
            return Err(e);
        }
        // 子文件全删空后，安全删除自身目录节点
        env.remove_dir(path)
    } else {
        // 如果是文件直接 unlink
        match env.unlink(path) {
            Ok(_) => Ok(()),
            Err(FsError::FileNotFound) if force => Ok(()), // -f 开启时，找不到文件不报错
            Err(e) => Err(e),
        }
    }
}

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();

    let mut force = false;
    let mut recursive = false;
    let mut targets = [""; 16];
    let mut target_count = 0;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'f' => force = true,
                    'r' | 'R' => recursive = true,
                    _ => {
                        env.write_stderr(b"Unknown option: -");
                        let mut char_buf = [0u8; 4];
                        let s = c.encode_utf8(&mut char_buf);
                        env.write_stderr(s.as_bytes());
                        env.write_stderr(b"\n");
                        env.exit(1);
                    }
                }
            }
        } else if target_count < 16 {
            targets[target_count] = arg;
            target_count += 1;
        }
    }

    if target_count == 0 {
        if force {
            // 如果 -f 开启，即使没有传入目标，也静默退出而不在控制台报错
            env.exit(0);
        }
        env.write_stderr(b"Usage: rm [-f] [-r] <file_path>\n");
        env.exit(1);
    }

    for i in 0..target_count {
        let target = targets[i];
        let res = if recursive {
            remove_recursive(&env, target, force)
        } else {
            match env.unlink(target) {
                Ok(_) => Ok(()),
                Err(FsError::FileNotFound) if force => Ok(()),
                Err(e) => Err(e),
            }
        };

        match res {
            Ok(_) => {}
            Err(FsError::FileNotFound) => {
                env.write_stderr(b"Error: File not found: ");
                env.write_stderr(target.as_bytes());
                env.write_stderr(b"\n");
                env.exit(1);
            }
            Err(FsError::PermissionDenied) => {
                env.write_stderr(b"Error: Permission denied\n");
                env.exit(1);
            }
            Err(_) => {
                env.write_stderr(b"Error: Unknown Error removing target: ");
                env.write_stderr(target.as_bytes());
                env.write_stderr(b"\n");
                env.exit(1);
            }
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 userspace/hnxlibc/src/lib.rs）。
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。argv 不通过
//    syscall 传递，所以这里 hardcode 一个固定路径。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    match remove_recursive(&env, "/testfile.txt", true) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}
