#[derive(Debug)]
pub enum ShellError {
    IoError,
    PathNotFound,
    PermissionDenied,
    Unknown,
}

pub trait Environment {
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError>;

    fn getcwd(&self, buf: &mut [u8]) -> Result<usize, ShellError>;
    fn chdir(&self, path: &str) -> Result<(), ShellError>;

    fn get_env(&self, key: &str, buf: &mut [u8]) -> Result<usize, ShellError>;
    fn set_env(&self, key: &str, value: &str) -> Result<(), ShellError>;
    fn print_envs(&self);

    // 运行外部程序 (SYSCALL_EXECVE -- replaces the calling process).
    // Returns the exit code captured at child exit time, or
    // `IoError` if the syscall itself failed.
    fn execute(&self, cmd: &str, args: &[&str]) -> Result<i32, ShellError>;

    // Spawn an EL0 process (SYSCALL_SPAWN -- does *not* replace
    // the calling process).  Returns the new pid as `Ok(pid)`
    // on success.
    //
    // B7: shell pipelines are realised by:
    //   1. env.pipe()  -> [read_fd, write_fd]
    //   2. env.spawn("cmd1", args1) -> pid1
    //   3. env.spawn("cmd2", args2) -> pid2
    //   4. env.dup2(read_fd, 0)      // cmd2 takes input from the pipe
    //   5. env.dup2(write_fd, 1)     // cmd1 sends output to the pipe
    //   6. (alternative: have the spawn call pass the fds
    //      via argv-as-arg-list; but we go with the simpler
    //      env-stateful variant for 1.0).
    fn spawn(&self, cmd: &str, args: &[&str]) -> Result<u64, ShellError>;
    fn pipe(&self, fds: &mut [i32; 2]) -> Result<(), ShellError>;
    fn dup2(&self, old_fd: i32, new_fd: i32) -> Result<(), ShellError>;
    fn wait(&self, pid: u64) -> Result<i32, ShellError>;
    fn yield_cpu(&self);
    fn open(&self, path: &str, flags: i32) -> Result<i32, ShellError>;
    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, ShellError>;

    // 读文件（供脚本解析使用）
    fn read_file(&self, path: &str, buf: &mut [u8]) -> Result<usize, ShellError>;

    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
