#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::FileSystem;

// 1. 无 std 下编译必须提供 panic_handler
#[cfg(not(feature = "host"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

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

// 3. Capsule OS 入口 (不使用标准库，不带 main，直接由 _start 进入)
#[cfg(not(feature = "host"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let env = env::capsule::CapsuleEnv;
    // 默认测试创建一个目录
    let _ = env.mkdir("/testdir");
    env.exit(0);
}
