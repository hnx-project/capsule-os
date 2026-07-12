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
//    `extern "Rust" { fn main() -> i32 }`（见 hnxlibc/src/lib.rs）。
//    我们不再自己实现 `_start` 和 `panic_handler`：这两者都由 hnxlibc
//    统一接管。argv 由 kernel 透传过来 (见 SYSCALL_EXECVE +
//    `kernel/src/syscall/handlers/process.rs::sys_execve`)，通过
//    hnxlibc 暴露的 `hnx_argc` / `hnx_arg` 读取。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    use hnxlibc::{hnx_arg, hnx_argc};
    let env = env::capsule::CapsuleEnv;
    let argc = hnx_argc();
    let mut recursive = false;
    let mut target: &[u8] = &[];
    for i in 1..argc as usize {
        let a = hnx_arg(i);
        if a == b"-p" {
            recursive = true;
        } else if target.is_empty() {
            target = a;
        }
    }
    if target.is_empty() {
        env.write_stderr(b"Usage: mkdir [-p] <directory_path>\n");
        return 1;
    }
    let path_str = match core::str::from_utf8(target) {
        Ok(s) => s,
        Err(_) => {
            env.write_stderr(b"Error: Invalid UTF-8 in argv\n");
            return 1;
        }
    };
    let res = if recursive {
        // TODO: 当 fileagent 真正起来后递归创建目录；现在 kernel 端只支持单层
        match env.mkdir(path_str) {
            Ok(_) => Ok(()),
            Err(env::FsError::AlreadyExists) => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        env.mkdir(path_str)
    };
    match res {
        Ok(_) => 0,
        Err(env::FsError::AlreadyExists) => {
            env.write_stderr(b"Error: Directory already exists\n");
            1
        }
        Err(env::FsError::PathNotFound) => {
            env.write_stderr(b"Error: Parent path not found\n");
            1
        }
        Err(env::FsError::PermissionDenied) => {
            env.write_stderr(b"Error: Permission denied\n");
            1
        }
        Err(_) => {
            env.write_stderr(b"Error: Unknown Error creating directory\n");
            1
        }
    }
}
