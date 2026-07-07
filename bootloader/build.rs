use std::env;
use std::path::PathBuf;

fn main() {
    // 获取当前 Cargo 正在编译的目标架构 (例如 aarch64, riscv64)
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    
    // 根据启用的 Feature 确定目标平台 (默认为 "virt")
    let platform = if env::var("CARGO_FEATURE_VIRT").is_ok() {
        "virt"
    } else {
        "virt" // 默认回退
    };

    // 动态拼接出对于该架构与该平台的 linker script 路径: src/linklds/{arch}/{platform}.ld
    let linker_script = format!("src/linklds/{}/{}.ld", target_arch, platform);
    
    // 告诉 Cargo 如果这个脚本或者 build.rs 改变了，就需要重新编译
    println!("cargo:rerun-if-changed={}", linker_script);
    println!("cargo:rerun-if-changed=build.rs");
    
    // 获取当前包的绝对路径
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let linker_script_path = PathBuf::from(manifest_dir).join(&linker_script);

    if !linker_script_path.exists() {
        panic!("Linker script not found at: {}", linker_script_path.display());
    }
    
    // 告诉 rustc 使用该路径的 linker script
    println!("cargo:rustc-link-arg=-T{}", linker_script_path.display());
}
