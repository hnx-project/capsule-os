use super::{ProcSystem, ProcessInfo};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn get_process_list(&self, _infos: &mut [ProcessInfo]) -> Result<usize, ()> {
        // TODO: 等待 SYSCALL_PROCESS_SNAPSHOT / 监控 channel 协议
        // （参见 userspace/programs/ps/src/capsule-design.md §1）。当前
        // kernel / fileagent 尚未落地进程快照通道，所以这里直接返回
        // `Ok(0)` 让 ps 在 shell 中显示空表。
        Ok(0)
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
