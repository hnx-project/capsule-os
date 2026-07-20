#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::{FileSystem, FsError};

/// 剥离尾部斜杠并获取父目录路径的简易算法 (no_std 兼容)
fn get_parent_dir(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(pos) = trimmed.rfind('/') {
        if pos == 0 {
            Some("/")
        } else {
            Some(&trimmed[..pos])
        }
    } else {
        // 无斜杠说明在当前目录下，父目录相当于为空
        None
    }
}

/// 级联递归删除空父目录的算法 (rmdir -p)
fn rmdir_recursive<F: FileSystem>(env: &F, path: &str) -> Result<(), FsError> {
    env.rmdir(path)?;
    if let Some(parent) = get_parent_dir(path) {
        if parent != "/" && !parent.is_empty() {
            let _ = rmdir_recursive(env, parent); // 级联静默向上删空目录
        }
    }
    Ok(())
}

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();

    let mut parents = false;
    let mut targets = [""; 16];
    let mut target_count = 0;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'p' => parents = true,
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
        env.write_stderr(b"Usage: rmdir [-p] <directory_path>\n");
        env.exit(1);
    }

    for i in 0..target_count {
        let target = targets[i];
        let res = if parents {
            rmdir_recursive(&env, target)
        } else {
            env.rmdir(target)
        };

        match res {
            Ok(_) => {}
            Err(env::FsError::DirectoryNotFound) => {
                env.write_stderr(b"Error: Directory not found: ");
                env.write_stderr(target.as_bytes());
                env.write_stderr(b"\n");
                env.exit(1);
            }
            Err(env::FsError::NotEmpty) => {
                env.write_stderr(b"Error: Directory not empty: ");
                env.write_stderr(target.as_bytes());
                env.write_stderr(b"\n");
                env.exit(1);
            }
            Err(env::FsError::PermissionDenied) => {
                env.write_stderr(b"Error: Permission denied\n");
                env.exit(1);
            }
            Err(_) => {
                env.write_stderr(b"Error: Unknown Error removing directory\n");
                env.exit(1);
            }
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 hnxlibc/src/lib.rs）。
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。argv 不通过
//    syscall 传递，所以这里 hardcode 一个固定路径。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    match rmdir_recursive(&env, "/testdir") {
        Ok(_) => 0,
        Err(_) => 1,
    }
}
