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

    // 运行外部程序
    fn execute(&self, cmd: &str, args: &[&str]) -> Result<i32, ShellError>;

    // 读文件（供脚本解析使用）
    fn read_file(&self, path: &str, buf: &mut [u8]) -> Result<usize, ShellError>;

    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
