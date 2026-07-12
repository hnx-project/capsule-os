#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::ProcSystem;

/// 解析极简字符串为数字而无需标准库支持 (no_std)
#[allow(dead_code)]
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0u32;
    for c in s.chars() {
        if c.is_ascii_digit() {
            val = val * 10 + (c as u32 - '0' as u32);
        } else {
            return None;
        }
    }
    Some(val)
}

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        env.write_stderr(b"Usage: kill <pid>\n");
        env.exit(1);
    }

    let pid_str = &args[1];
    match parse_u32(pid_str) {
        Some(pid) => {
            match env.kill(pid, 15) {
                // 15 = SIGTERM
                Ok(_) => {
                    env.write_stdout(b"Process ");
                    env.write_stdout(pid_str.as_bytes());
                    env.write_stdout(b" terminated.\n");
                }
                Err(env::KillError::ProcessNotFound) => {
                    env.write_stderr(b"Error: Process not found: ");
                    env.write_stderr(pid_str.as_bytes());
                    env.write_stderr(b"\n");
                    env.exit(1);
                }
                Err(env::KillError::PermissionDenied) => {
                    env.write_stderr(b"Error: Permission denied\n");
                    env.exit(1);
                }
                Err(_) => {
                    env.write_stderr(b"Error: Unknown kill error\n");
                    env.exit(1);
                }
            }
        }
        None => {
            env.write_stderr(b"Error: Invalid PID: ");
            env.write_stderr(pid_str.as_bytes());
            env.write_stderr(b"\n");
            env.exit(1);
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 hnxlibc/src/lib.rs）。
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。
//
//    TODO: argv 解析依赖 kernel 把 exec 的 cmd line 透传给 target program，
//    目前 exec 不带 argv，所以这里 hardcode 给 pid=10 发 SIGTERM 占位。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    match env.kill(10, 15) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
