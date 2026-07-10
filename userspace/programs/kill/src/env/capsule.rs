use super::{KillError, ProcSystem};

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn kill(&self, _pid: u32, _signal: i32) -> Result<(), KillError> {
        // TODO: 对接 hnxlibc::kill(_pid, _signal) 或相关的系统调用通道
        Ok(())
    }

    fn write_stdout(&self, _data: &[u8]) {
        // TODO: 对接 hnxlibc::write(1, _data)
    }

    fn write_stderr(&self, _data: &[u8]) {
        // TODO: 对接 hnxlibc::write(2, _data)
    }

    fn exit(&self, _code: i32) -> ! {
        // TODO: 对接 hnxlibc::exit(_code)
        loop {}
    }
}
