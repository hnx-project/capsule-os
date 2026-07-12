#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;
pub mod ls;
pub mod options;

use crate::env::FileSystem;
use crate::options::LsOptions;

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();

    let mut opts = LsOptions::new();
    let mut target_dir = ".";

    // 解析非常基础的命令行参数而无需引入额外的 clap 库 (no_std 友好)
    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'a' => opts.all = true,
                    'l' => opts.long = true,
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
        } else {
            target_dir = arg;
        }
    }

    match ls::run_ls(&env, target_dir, &opts) {
        Ok(_) => {}
        Err(env::FsError::DirectoryNotFound) => {
            env.write_stderr(b"Error: Directory not found: ");
            env.write_stderr(target_dir.as_bytes());
            env.write_stderr(b"\n");
            env.exit(1);
        }
        Err(env::FsError::PermissionDenied) => {
            env.write_stderr(b"Error: Permission denied\n");
            env.exit(1);
        }
        Err(_) => {
            env.write_stderr(b"Error: Unknown FS Error\n");
            env.exit(1);
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 hnxlibc/src/lib.rs）。
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。
//
//    TODO: 真正的目录遍历依赖 SYSCALL_READDIR + Dirent 协议
//    （参见 userspace/programs/ls/src/capsule-design.md §2），目前
//    kernel / fileagent 还没落地，所以这里只 emit 一条占位提示让 ls
//    能 boot 起来，等 readdir 协议补齐后接入 `ls::run_ls`。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    env.write_stderr(b"ls: readdir not yet wired in this CapsuleOS build\n");
    let _ = env.exit(1);
}
