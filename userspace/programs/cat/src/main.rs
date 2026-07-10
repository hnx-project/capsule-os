#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod cat;
pub mod env;
pub mod options;

use crate::env::FileSystem;
use crate::options::CatOptions;

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
    
    let mut opts = CatOptions::new();
    let mut file_path = "";
    let mut file_specified = false;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'n' => opts.number = true,
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
            file_path = arg;
            file_specified = true;
        }
    }

    if !file_specified {
        env.write_stderr(b"Usage: cat [-n] <file_path>\n");
        env.exit(1);
    }

    match cat::run_cat(&env, file_path, &opts) {
        Ok(_) => {}
        Err(env::FsError::FileNotFound) => {
            env.write_stderr(b"Error: File not found: ");
            env.write_stderr(file_path.as_bytes());
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

// 3. Capsule OS 入口 (不使用标准库，不带 main，直接由 _start 进入)
#[cfg(not(feature = "host"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let env = env::capsule::CapsuleEnv;
    let opts = CatOptions::new();
    // 默认以标准默认配置浏览
    let _ = cat::run_cat(&env, "/system/test.txt", &opts);
    env.exit(0);
}
