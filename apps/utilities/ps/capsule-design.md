# ps / kill 与 CapsuleOS 进程监控对接设计方案

本篇设计方案规范了 `ps` 与 `kill` 工具与 CapsuleOS 进程调度服务、监控进程管理器进行底层对接的技术细节，确保在 `no_std` 下用户态对多任务流控制的安全平稳落地。

---

## 1. 进程快照获取 (ps 核心机制)

在一个安全的多任务操作系统中，进程信息是不允许通过物理内存直接任意跨界读取的。`ps` 进程索取快照的推荐微内核方案如下：

1. **注册专属监控 Channel**：系统启动后，微内核会在用户态常驻一个系统监控服务进程（或由初始化服务进程 `init` 代理）。
2. **消息拉取列表**：
   * `ps` 工具启动后，利用 `channel_lookup("svc.process")` 找到系统管理器专属句柄。
   * 发送快照索取请求包。
   * 系统管理器打包当前活动的 PCB 快照，以 flat 平坦字节缓存形式回传给 `ps` 的 `get_process_list`。
   * `ps` 解析该定长结构并高亮优雅打印。

---

## 2. 终止信号传递 (kill 核心机制)

* **机制原理**：`kill` 工具主要负责向特定的 `PID` 发送预警信号或强退指令。
* **系统调用**：`SYSCALL_EXIT` 仅支持自己进程主动退出。强退外部进程必须由特权用户态（或通过内核系统调用）向进程调度器请求。
* **推荐接口**：在 `hnxlibc` 中新增信号分发调用：
  ```rust
  // 向指定 PID 进程投递终止信号。如 sig = 15 (SIGTERM)
  pub fn kill(pid: u32, sig: i32) -> i32;
  ```
  该接口最终通过微内核系统调用向目标 PCB 投递一个“终止（Abort）”的软件中断或异步信号。

---

## 3. 对接代码最佳实践

### A. `ps` 对接实现
```rust
use super::{ProcSystem, ProcessInfo};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn get_process_list(&self, infos: &mut [ProcessInfo]) -> Result<usize, ()> {
        let ptr = infos.as_mut_ptr() as *mut u8;
        // 假定 hnxlibc 导出了 get_process_snapshot 接口
        let res = hnxlibc::get_process_snapshot(ptr, infos.len());
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(())
        }
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
```

### B. `kill` 对接实现
```rust
use super::{KillError, ProcSystem};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn kill(&self, pid: u32, signal: i32) -> Result<(), KillError> {
        let res = hnxlibc::kill(pid, signal);
        if res == 0 {
            Ok(())
        } else if res == -3 { // ESRCH (ProcessNotFound)
            Err(KillError::ProcessNotFound)
        } else if res == -1 { // EPERM (PermissionDenied)
            Err(KillError::PermissionDenied)
        } else {
            Err(KillError::Unknown)
        }
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
```
