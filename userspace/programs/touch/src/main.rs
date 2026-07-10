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
    
    let mut no_create = false;
    let mut targets = [""; 16];
    let mut target_count = 0;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'c' => no_create = true,
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
        env.write_stderr(b"Usage: touch [-c] <file_path>\n");
        env.exit(1);
    }

    for i in 0..target_count {
        let target = targets[i];
        match env.touch(target, no_create) {
            Ok(_) => {}
            Err(env::FsError::PathNotFound) => {
                env.write_stderr(b"Error: Parent path not found: ");
                env.write_stderr(target.as_bytes());
                env.write_stderr(b"\n");
                env.exit(1);
            }
            Err(env::FsError::PermissionDenied) => {
                env.write_stderr(b"Error: Permission denied\n");
                env.exit(1);
            }
            Err(_) => {
                env.write_stderr(b"Error: Unknown Error creating file\n");
                env.exit(1);
            }
        }
    }
}

// 3. Capsule OS 入口 (不使用标准库，不带 main，直接由 _start 进入)
#[cfg(not(feature = "host"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let env = env::capsule::CapsuleEnv;
    let _ = env.touch("/testfile.txt", false);
    env.exit(0);
}
