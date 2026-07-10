#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::{ProcSystem, ProcessInfo};

// 1. 无 std 下编译必须提供 panic_handler
#[cfg(not(feature = "host"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// 格式化数字到 byte 数组中而无需任何堆内存分配 (no_std 兼容)
fn format_u32(mut val: u32, buf: &mut [u8]) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut temp = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        temp[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    let mut j = 0;
    while i > 0 {
        i -= 1;
        buf[j] = temp[i];
        j += 1;
    }
    j
}

// 2. 本地测试与 Capsule OS 的通用打印控制逻辑
fn print_process_table<P: ProcSystem>(env: &P) {
    let mut list = [ProcessInfo::new(); 32];
    match env.get_process_list(&mut list) {
        Ok(count) => {
            env.write_stdout(b"  PID  PPID  STATUS  COMMAND\n");
            for i in 0..count {
                let info = &list[i];

                let mut buf = [0u8; 10];
                // PID 对齐格式化 (4位宽)
                let pid_len = format_u32(info.pid, &mut buf);
                let pid_padding = 5 - pid_len.min(5);
                for _ in 0..pid_padding {
                    env.write_stdout(b" ");
                }
                env.write_stdout(&buf[..pid_len]);
                env.write_stdout(b"  ");

                // PPID 对齐格式化 (4位宽)
                let ppid_len = format_u32(info.ppid, &mut buf);
                let ppid_padding = 4 - ppid_len.min(4);
                for _ in 0..ppid_padding {
                    env.write_stdout(b" ");
                }
                env.write_stdout(&buf[..ppid_len]);
                env.write_stdout(b"  ");

                // STATUS 翻译
                let state_str = match info.state {
                    1 => b"RUN     ",
                    2 => b"SLEEP   ",
                    3 => b"ZOMBIE  ",
                    _ => b"UNKNOWN ",
                };
                env.write_stdout(state_str);

                // COMMAND 打印
                env.write_stdout(&info.name[..info.name_len]);
                env.write_stdout(b"\n");
            }
        }
        Err(_) => {
            env.write_stderr(b"Error: Failed to obtain process snapshot\n");
        }
    }
}

// 3. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    print_process_table(&env);
}

// 4. Capsule OS 入口 (不使用标准库，不带 main，直接由 _start 进入)
#[cfg(not(feature = "host"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let env = env::capsule::CapsuleEnv;
    print_process_table(&env);
    env.exit(0);
}
