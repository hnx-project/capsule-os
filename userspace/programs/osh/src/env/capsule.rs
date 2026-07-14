use super::{Environment, ShellError};

extern crate hnxlibc;
extern crate hnxstd;

pub struct CapsuleEnv;

/// Capsule OS 下的 PAL 实现.
///
/// 该实现把 osh 的 Environment trait 直接桥接到 `hnxlibc` 与 `hnxstd`：
/// - 标准 I/O 调用 hnxstd 统一提供的底层标准接口
/// - 外部进程执行走 SYSCALL_EXEC
/// - 当前目录、环境变量、文件读取等还未在 CapsuleOS 中实现的子系统
///   一律返回占位 Ok/Err，这样 osh 至少能进入 REPL 并执行内置命令，
///   后续在内核补齐 SYSCALL_GETCWD / SYSCALL_CHDIR 后只需修改本文件。
impl Environment for CapsuleEnv {
    fn write_stdout(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            hnxstd::io::print(s);
        }
    }

    fn write_stderr(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            hnxstd::io::print(s);
        }
    }

    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        // Clean buffer first to prevent dirty remnants
        for i in 0..buf.len() {
            buf[i] = 0;
        }

        let mut res = hnxlibc::read(0, buf.as_mut_ptr(), buf.len());

        // Adaptively reconstruct actual read length if return value is clobbered to 0
        if res == 0 {
            let mut count = 0;
            while count < buf.len() && buf[count] != 0 && buf[count] != b'\n' {
                count += 1;
            }
            if count < buf.len() && buf[count] == b'\n' {
                res = (count + 1) as isize;
            }
        }

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

    fn execute(&self, cmd: &str, args: &[&str]) -> Result<i32, ShellError> {
        // Forward to SYSCALL_EXECVE with argv[0]=cmd, argv[1..]=args.
        // The kernel materialises argv on the child process's user stack
        // and hands argc / argv in x0/x1 at entry; see
        // `kernel/src/syscall/handlers/process.rs::sys_execve` and the
        // hnxlibc user entry trampoline in `hnxlibc/src/lib.rs`.
        let mut argv_storage: [&[u8]; 16] = [&[]; 16];
        argv_storage[0] = cmd.as_bytes();
        for (i, a) in args.iter().enumerate() {
            if i + 1 >= 16 {
                break;
            }
            argv_storage[i + 1] = a.as_bytes();
        }
        let count = core::cmp::min(args.len() + 1, 16);
        let code = hnxlibc::execve(cmd, &argv_storage[..count]);
        Ok(code)
    }

    fn spawn(&self, cmd: &str, args: &[&str]) -> Result<u64, ShellError> {
        // SYSCALL_SPAWN: launches cmd without replacing the
        // caller.  The new process's stdout inherits osh's
        // pipe fd (if any); see B7 shell.rs for how the
        // pipeline sets the dup2 first.
        let mut argv_storage: [&[u8]; 16] = [&[]; 16];
        argv_storage[0] = cmd.as_bytes();
        for (i, a) in args.iter().enumerate() {
            if i + 1 >= 16 {
                break;
            }
            argv_storage[i + 1] = a.as_bytes();
        }
        let count = core::cmp::min(args.len() + 1, 16);
        match hnxlibc::spawn(cmd, &argv_storage[..count]) {
            Ok(pid) => Ok(pid),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn pipe(&self, fds: &mut [i32; 2]) -> Result<(), ShellError> {
        // SYSCALL_PIPE writes its two fds into the caller's
        // user memory at `fds.as_mut_ptr()`; we use a
        // stack-array since the kernel reads 8 bytes (two
        // i32s).
        let mut raw = [0i32; 2];
        match hnxlibc::pipe_pair(&mut raw) {
            Ok(()) => {
                fds[0] = raw[0];
                fds[1] = raw[1];
                Ok(())
            }
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn dup2(&self, old_fd: i32, new_fd: i32) -> Result<(), ShellError> {
        match hnxlibc::dup2(old_fd, new_fd) {
            Ok(_) => Ok(()),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn wait(&self, pid: u64) -> Result<i32, ShellError> {
        let mut status: i32 = 0;
        loop {
            // Restore status pointer back to &mut status to safely return wait codes
            match hnxlibc::wait4(pid as i64, &mut status, 0) {
                Ok((_, _)) => return Ok(status),
                Err(hnxlibc::Status::TryAgain) => {
                    // Child is still running, yield our remaining quantum
                    hnxlibc::yield_cpu();
                }
                Err(_) => return Err(ShellError::IoError),
            }
        }
    }

    fn yield_cpu(&self) {
        hnxlibc::yield_cpu();
    }

    fn open(&self, path: &str, flags: i32) -> Result<i32, ShellError> {
        match hnxlibc::open_str(path, flags, 0) {
            fd if fd >= 0 => Ok(fd),
            _ => Err(ShellError::PathNotFound),
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, ShellError> {
        let n = hnxlibc::read(fd, buf.as_mut_ptr(), buf.len());
        if n >= 0 {
            Ok(n as usize)
        } else {
            Err(ShellError::IoError)
        }
    }

    fn write(&self, fd: i32, data: &[u8]) -> Result<usize, ShellError> {
        let n = hnxlibc::write(fd, data.as_ptr(), data.len());
        if n >= 0 {
            Ok(n as usize)
        } else {
            Err(ShellError::IoError)
        }
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
