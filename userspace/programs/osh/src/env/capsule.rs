use super::{Environment, ShellError};

extern crate hnxlibc;

pub struct CapsuleEnv;

/// Capsule OS 下的 PAL 实现.
///
/// 该实现把 osh 的 Environment trait 直接桥接到 `hnxlibc`：
/// - 标准 I/O 走 EL0 直接 syscall (PL011 UART)
/// - 外部进程执行走 SYSCALL_EXEC
/// - 当前目录、环境变量、文件读取等还未在 CapsuleOS 中实现的子系统
///   一律返回占位 Ok/Err，这样 osh 至少能进入 REPL 并执行内置命令，
///   后续在内核补齐 SYSCALL_GETCWD / SYSCALL_CHDIR 后只需修改本文件。
impl Environment for CapsuleEnv {
    fn write_stdout(&self, data: &[u8]) {
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        // fd = 0 标准输入。kernel sys_read 在 fd=0 时阻塞读 PL011/NS16550
        // UART 直到换行 (见 kernel/src/syscall/handlers/vfs.rs)。
        let res = hnxlibc::read(0, buf.as_mut_ptr(), buf.len());
        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(ShellError::IoError)
        }
    }

    fn getcwd(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        match hnxlibc::getcwd(buf) {
            Ok(n) => Ok(n),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn chdir(&self, path: &str) -> Result<(), ShellError> {
        match hnxlibc::chdir(path) {
            Ok(()) => Ok(()),
            Err(_) => Err(ShellError::PathNotFound),
        }
    }

    fn get_env(&self, _key: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // TODO: 等待 SYSCALL_GETENV 在 kernel 端落地。
        Err(ShellError::PathNotFound)
    }

    fn set_env(&self, _key: &str, _value: &str) -> Result<(), ShellError> {
        // TODO: 等待 SYSCALL_SETENV 在 kernel 端落地。
        Ok(())
    }

    fn print_envs(&self) {
        // TODO: 等待环境变量子系统就绪。
    }

    fn execute(&self, cmd: &str, _args: &[&str]) -> Result<i32, ShellError> {
        // hnxlibc::exec 在内部走 SYSCALL_EXEC；CapsuleOS 暂时不向子进程
        // 传递 argv，因此这里只转发 cmd name。
        let code = hnxlibc::exec(cmd);
        Ok(code)
    }

    fn read_file(&self, _path: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // TODO: 等待 fileagent (svc.vfs) 被 init 拉起并注册到 IPC 总线后，
        //       再串 open(path, O_RDONLY, 0) + read(fd, buf, buf.len()) + close(fd)。
        Err(ShellError::IoError)
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
