use super::{ProcSystem, ProcessInfo};

extern crate libstd;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn get_process_list(&self, _infos: &mut [ProcessInfo]) -> Result<usize, ()> {
        Ok(0)
    }

    fn write_stdout(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn write_stderr(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn exit(&self, _code: i32) -> ! {
        panic!("Process exited");
    }
}
