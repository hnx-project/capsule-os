#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::FileSystem;

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        env.write_stderr(b"Usage: mkdir <directory_path>\n");
        env.exit(1);
    }

    // 支持解析极简的 -p 参数，如果是 -p，则在 host 端自动递归创建
    let mut recursive = false;
    let mut target_dir = "";
    for arg in args.iter().skip(1) {
        if arg == "-p" {
            recursive = true;
        } else {
            target_dir = arg;
        }
    }

    if target_dir.is_empty() {
        env.write_stderr(b"Usage: mkdir [-p] <directory_path>\n");
        env.exit(1);
    }

    let res = if recursive {
        // 在 host 端递归创建目录以完美运行脚本测试
        match std::fs::create_dir_all(target_dir) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                std::io::ErrorKind::PermissionDenied => Err(env::FsError::PermissionDenied),
                _ => Err(env::FsError::Unknown),
            },
        }
    } else {
        env.mkdir(target_dir)
    };

    match res {
        Ok(_) => {}
        Err(env::FsError::AlreadyExists) => {
            env.write_stderr(b"Error: Directory already exists: ");
            env.write_stderr(target_dir.as_bytes());
            env.write_stderr(b"\n");
            env.exit(1);
        }
        Err(env::FsError::PathNotFound) => {
            env.write_stderr(b"Error: Parent path not found\n");
            env.exit(1);
        }
        Err(env::FsError::PermissionDenied) => {
            env.write_stderr(b"Error: Permission denied\n");
            env.exit(1);
        }
        Err(_) => {
            env.write_stderr(b"Error: Unknown Error creating directory\n");
            env.exit(1);
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 userspace/hnxlibc/src/lib.rs）。
//    我们不再自己实现 `_start` 和 `panic_handler`：这两者都由 hnxlibc
//    统一接管。argv 不通过 syscall 传递（kernel exec 当前不传递 argv），
//    所以这里 hardcode 一个固定路径让 mkdir 的 EL0 入口能跑通。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    match env.mkdir("/testdir") {
        Ok(_) => 0,
        Err(_) => 1,
    }
}
