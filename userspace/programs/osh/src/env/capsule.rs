use super::{Environment, ShellError};

pub struct CapsuleEnv;

// 这里为 Capsule OS no_std 做一个最小可行性占位实现。
// 在未来将 hnxlibc 接入时，直接用对应的系统调用函数填充这里即可。
impl Environment for CapsuleEnv {
    fn write_stdout(&self, _data: &[u8]) {
        // TODO: 后面对接 hnxlibc::sys_write(1, _data)
    }

    fn write_stderr(&self, _data: &[u8]) {
        // TODO: 后面对接 hnxlibc::sys_write(2, _data)
    }

    fn read_line(&self, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // TODO: 后面对接 hnxlibc::sys_read(0, _buf)
        Ok(0)
    }

    fn getcwd(&self, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // TODO: 后面对接 hnxlibc::sys_getcwd(_buf)
        Ok(0)
    }

    fn chdir(&self, _path: &str) -> Result<(), ShellError> {
        // TODO: 后面对接 hnxlibc::sys_chdir(_path)
        Ok(())
    }

    fn get_env(&self, _key: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // TODO: 后面对接 hnxlibc::sys_getenv(_key, _buf)
        Ok(0)
    }

    fn set_env(&self, _key: &str, _value: &str) -> Result<(), ShellError> {
        // TODO: 后面对接 hnxlibc::sys_setenv(_key, _value)
        Ok(())
    }

    fn print_envs(&self) {
        // TODO: 遍历打印环境变量
    }

    fn execute(&self, _cmd: &str, _args: &[&str]) -> Result<i32, ShellError> {
        // TODO: 对接 hnxlibc::exec(_cmd) 或采用系统的 channel 发送加载程序指令
        Ok(0)
    }

    fn read_file(&self, _path: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // TODO: 对接 hnxlibc::open 与 hnxlibc::read
        Ok(0)
    }

    fn exit(&self, _code: i32) -> ! {
        // TODO: 后面对接 hnxlibc::sys_exit(_code)
        loop {}
    }
}
