# Capsule OS 用户态生态后续演进与集成规划蓝图 (future.md)

本篇文档为 `capsule-os` 用户态应用集（八大金刚：`osh`, `ls`, `cat`, `mkdir`, `touch`, `rm`, `rmdir`, `ps`, `kill`）的未来集成与 Zsh 级高阶演进提供了系统级的指导方案。

---

## 一、 系统级构建与打包集成 (xtask 接入)

为了将这 8 个独立的 Rust 用户态项目自动化编译并打入系统的根文件系统镜像，我们需要在 `capsule-os` 的构建脚本中建立级联编译机制。

### 1.1 xtask 级联交叉编译流
在 `capsule-os/tools/xtask/src/build.rs` 中，新增对外部程序项目的构建调度逻辑：
```rust
// 示例：xtask 用户态级联编译流
fn build_userspace_programs() -> Result<(), xShellError> {
    let programs = ["osh", "ls", "cat", "mkdir", "touch", "rm", "rmdir", "ps", "kill"];
    for prog in programs {
        let path = format!("../programs/{}", prog);
        cmd!("cargo build")
            .env("CARGO_ENCODING", "UTF-8")
            .args(&[
                "--manifest-path", &format!("{}/Cargo.toml", path),
                "--target", "riscv64-unknown-capsule", // 或 aarch64
                "--no-default-features",
                "--features", "capsule",
                "--release"
            ])
            .run()?;
            
        // 复制 ELF 映像到 staging_rootfs 对应的 system/bin 目录下
        let src_binary = format!("{}/target/riscv64-unknown-capsule/release/{}", path, prog);
        let dest_binary = format!("build/dist/staging_rootfs/system/bin/{}", prog);
        std::fs::copy(src_binary, dest_binary)?;
    }
    Ok(())
}
```

### 1.2 系统默认 Shell (osh) 的自启动
在 `capsule-os/userspace/services/init/src/main.rs` 中，设备树与文件系统初始化成功后，通过 `exec` 替换为默认 Shell：
```rust
// init 服务拉起默认 Shell
fn launch_default_shell() -> ! {
    let shell_path = "/system/bin/osh";
    // 微内核 execve 调度
    hnxlibc::exec(shell_path);
    loop {}
}
```

---

## 二、 hnxlibc 底层接口补齐计划 (VFS 与 信号)

参考我们为每个程序制定的 `capsule-design.md` 细节规范，`hnxlibc` 需要在后续版本中提供并封装以下微内核调用及数据结构。

### 2.1 目录检索协议 (ls 对接)
* **VFS 指令扩充**：在 `FileAgentCmd` 枚举中扩充 `ReadDir` 并定义 128 字节物理对齐的 `Dirent`。
```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent {
    pub ino: u64,        // 8 字节
    pub size: u64,       // 8 字节
    pub ftype: u8,       // 1 字节 (0 = 未知, 1 = 普通文件, 2 = 目录)
    pub name_len: u8,    // 1 字节
    pub name: [u8; 110], // 110 字节 (总共 128 字节)
}
```
* **接口导出**：
  ```rust
  pub fn readdir(fd: i32, dirent_ptr: *mut u8) -> i32;
  ```

### 2.2 元数据删除协议 (rm / rmdir 对接)
* **接口导出**：
  ```rust
  pub fn unlink(path: *const u8) -> i32;
  pub fn rmdir(path: *const u8) -> i32;
  ```

### 2.3 任务控制及监控协议 (ps / kill 对接)
* **进程快照机制**：进程监控管理器需要建立 PCB 数据平坦化机制。
```rust
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub state: u8,
    pub name: [u8; 16],
    pub name_len: usize,
}
```
* **接口导出**：
  ```rust
  pub fn get_process_snapshot(buf_ptr: *mut u8, max_count: usize) -> i32;
  pub fn kill(pid: u32, sig: i32) -> i32;
  ```

---

## 三、 Zsh 级交互特性演进 (Pure Userspace ZLE)

在无需改动内核的前提下，`osh` 计划通过用户态纯代码状态机进行丝滑 TTY 的独立演进。

### 3.1 极简行编辑器 (Zsh Line Editor 雏形)
1. **字符级原始拦截**：
   * 将终端 TTY 设置为 `Raw Mode`（通过 `hnxlibc` 或 ioctl 信道配置）。
   * 采用无阻塞/阻塞的逐字符 `getchar`。
2. **重绘与退格**：
   * 在 `osh` 进程内存中维护一个环形命令历史 `HistoryBuffer` 和光标相对位移 `cursor`。
   * 拦截到 `BackSpace (\x7f)`、`\x1b[C`（右方向）与 `\x1b[D`（左方向）等控制字符后，通过发送 `\x1b[K`（清除行）、`\x1b[s` / `\x1b[u`（光标现场恢复）自主代理回显。
3. **Tab 前缀自动补全**：
   * 拦截到 `\x09` (Tab 键)，提取当前行尾部字串。
   * 发起目录读取（`readdir`）协议，进行无分配迭代器前缀过滤（`starts_with`）。
   * 有唯一项时自动覆写缓冲光标并回显。

### 3.2 变量与脚本 AST 解释器 (Zsh Script Interpreter)
1. **Token 零分配借用**：
   * 词法分析器 `Lexer<'a>` 在提取类似 `&&`、`|`、`>` 符号时，仅保留原始行的切片引用（零堆分配），消除内存碎片和 OOM 崩溃隐患。
2. **Flat-Arena AST（抽象语法树）设计**：
   * 弃用 `Box<Node>` 的经典指针模式，通过静态一维数组 `[Option<AstNode>; 32]` 进行索引级连寻址。
   * 纯物理栈递归完成条件分支（`IfStmt`）、循环（`WhileStmt`）和重定向管道在用户态的最轻量化调度执行。
