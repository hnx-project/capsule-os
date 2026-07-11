#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

#[cfg(not(feature = "host"))]
use hnxlibc::{hnx_arg, hnx_argc, write as hwrite};

pub mod builtins;
pub mod env;
pub mod parser;
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
//    `extern "Rust" { fn main() -> i32 }`（见 userspace/hnxlibc/src/lib.rs）。
//    我们把 REPL 入口挂到 `pub fn main` 上，让 hnxlibc 接管栈对齐、RA 清零
//    和退出码回传。不需要我们自己的 `#[panic_handler]`：hnxlibc 已注册一个。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    // Print the argv the kernel handed us via the hnxlibc entry
    // trampoline.  This is the user-space end of the
    // `sys_execve` -> kernel argv copy -> user stack ->
    // `_hnx_user_entry` -> `__HNX_ARGV_*` globals -> `hnx_arg*`
    // handoff, so it makes the whole path end-to-end observable in
    // the boot log.  Anything other than "argc=N argv[0]=osh
    // argv[1]=init-was-here" in the output would mean argv is
    // being dropped or corrupted somewhere on the way through.
    print_argv_summary();
    let env = env::capsule::CapsuleEnv;
    shell::run_shell(&env)
}

/// Print `argc` and each `argv[i]` to stdout, one per line, so the
/// kernel -> user-space argv handoff is visible in the boot log.
/// Uses `hnxlibc::write` directly (not the `env` trait) because this
/// is a Capsule-OS-only path and host-mode tests should not be
/// coupled to a feature that exists only to verify a kernel ABI.
#[cfg(not(feature = "host"))]
fn print_argv_summary() {
    let argc = hnx_argc();
    hwrite_str("osh: argc=");
    print_dec(argc as u32);
    hwrite_str("\n");
    for i in 0..argc as usize {
        hwrite_str("osh: argv[");
        print_dec(i as u32);
        hwrite_str("]=");
        let arg = hnx_arg(i);
        hwrite(1, arg.as_ptr(), arg.len());
        hwrite_str("\n");
    }
}

#[cfg(not(feature = "host"))]
fn hwrite_str(s: &str) {
    hwrite(1, s.as_ptr(), s.len());
}

#[cfg(not(feature = "host"))]
fn print_dec(mut n: u32) {
    if n == 0 {
        hwrite_str("0");
        return;
    }
    // u32::MAX is 10 digits; 12 bytes is comfortable headroom.
    let mut buf = [0u8; 12];
    let mut i: usize = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    // Reverse in place.
    let mut lo: usize = 0;
    let mut hi = i - 1;
    while lo < hi {
        let t = buf[lo];
        buf[lo] = buf[hi];
        buf[hi] = t;
        lo += 1;
        hi -= 1;
    }
    hwrite(1, buf.as_ptr(), i);
}
