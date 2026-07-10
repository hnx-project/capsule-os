use super::{KillError, ProcSystem};
use std::io::Write;

pub struct HostEnv;

impl ProcSystem for HostEnv {
    fn kill(&self, pid: u32, _signal: i32) -> Result<(), KillError> {
        // 在 host 上调用标准库的 kill 模拟
        // 我们通过 libc 或者是 std::process::Command 调用本地终端命令
        let mut command = std::process::Command::new("kill");
        // 传递 SIGTERM (15) 或指定参数
        command.arg(pid.to_string());
        
        match command.status() {
            Ok(status) if status.success() => Ok(()),
            _ => Err(KillError::ProcessNotFound),
        }
    }

    fn write_stdout(&self, data: &[u8]) {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(data);
        let _ = stdout.flush();
    }

    fn write_stderr(&self, data: &[u8]) {
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(data);
        let _ = stderr.flush();
    }

    fn exit(&self, code: i32) -> ! {
        std::process::exit(code);
    }
}
