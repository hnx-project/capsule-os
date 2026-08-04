#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

#[cfg(not(feature = "host"))]
extern crate libstd;

pub mod builtins;
pub mod env;
pub mod parser;
pub mod readline;
pub mod shell;

// 1. 本地测试与调试环境入口（使用标准库）
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        // 如果有参数，视为脚本路径，进入非交互式脚本执行模式
        shell::run_script(&env, &args[1]);
    } else {
        // 否则进入交互式 REPL 命令行
        shell::run_shell(&env);
    }
}

// 2. Capsule OS 用户态入口：hnxlibc 在 _start 之后会调用
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。不需要我们自己的 `#[panic_handler]`。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    // Boot cleanup: clear the screen so the kernel boot log
    // (replayed by svc.tty via dmesg) gives way to the shell
    // prompt.  This is what every commercial OS does at init time
    // (Linux `getty` issues a screen clear, systemd sets a quiet
    // plymouth splash, FreeBSD's `cons25` driver clears on login).
    libstd::io::print("\x1b[2J\x1b[H");
    print_argv_summary();
    libstd::io::print("CapsuleOS Pangu v1.0.0  --  osh shell\n");
    libstd::io::print("Type 'help' for built-ins, 'dmesg' to read the kernel ring buffer.\n\n");
    let env = env::capsule::CapsuleEnv;
    shell::run_shell(&env);
    0
}

/// Print `argc` and each `argv[i]` to stdout, one per line, so the
/// kernel -> user-space argv handoff is visible in the boot log.
#[cfg(not(feature = "host"))]
fn print_argv_summary() {
    let mut s_argc = libstd::string::String::from_str("osh: argc=");
    libstd::io::print(s_argc.as_str());

    // Iterate arguments using the standard Iterator!
    let args_iter = libstd::env::args();
    let mut count = 0;
    for arg in args_iter {
        let mut prefix = libstd::string::String::from_str("osh: argv[");
        // Simple manual counter append
        let _ = prefix.push_byte(b'0' + (count % 10) as u8);
        let _ = prefix.push_str("]=");
        libstd::io::print(prefix.as_str());
        libstd::io::print(arg.as_str());
        libstd::io::print("\n");
        count += 1;
    }
}
