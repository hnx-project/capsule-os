use super::{ProcSystem, ProcessInfo};

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn get_process_list(&self, _infos: &mut [ProcessInfo]) -> Result<usize, ()> {
        // TODO: 后面对接 hnxlibc::get_process_snapshot(_infos) 或特定监控 channel 通讯
        Ok(0)
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
