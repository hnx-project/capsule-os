#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod builtins;
pub mod env;
pub mod parser;
pub mod shell;

// 如果没有 host 特性，则由 no_std 生态接管，必须提供 panic 处理器
#[cfg(not(feature = "host"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

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

// 2. Capsule OS 用户态入口（不使用标准库，不带 main，直接由 _start 进入）
#[cfg(not(feature = "host"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let env = env::capsule::CapsuleEnv;
    shell::run_shell(&env);
}
