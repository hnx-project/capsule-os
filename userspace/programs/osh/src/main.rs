#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

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
    let env = env::capsule::CapsuleEnv;
    shell::run_shell(&env);
}
