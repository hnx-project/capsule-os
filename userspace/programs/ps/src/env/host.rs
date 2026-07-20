use super::{ProcSystem, ProcessInfo};
use std::io::Write;

pub struct HostEnv;

impl ProcSystem for HostEnv {
    fn get_process_list(&self, infos: &mut [ProcessInfo]) -> Result<usize, ()> {
        // 由于在 macOS/Linux 上不便无分配极速调取原生 C 库获取列表，
        // 我们在此采用一份精美的高保真主机仿真实现，模拟系统内典型的进程分布结构。
        if infos.len() < 4 {
            return Err(());
        }

        // 1. init 进程
        infos[0] = ProcessInfo {
            pid: 1,
            ppid: 0,
            state: 1, // Running
            name: {
                let mut n = [0u8; 16];
                n[..4].copy_from_slice(b"init");
                n
            },
            name_len: 4,
        };

        // 2. devmgr 进程
        infos[1] = ProcessInfo {
            pid: 2,
            ppid: 1,
            state: 2, // Sleeping
            name: {
                let mut n = [0u8; 16];
                n[..6].copy_from_slice(b"devmgr");
                n
            },
            name_len: 6,
        };

        // 3. fileagent (VFS)
        infos[2] = ProcessInfo {
            pid: 3,
            ppid: 1,
            state: 2, // Sleeping
            name: {
                let mut n = [0u8; 16];
                n[..9].copy_from_slice(b"fileagent");
                n
            },
            name_len: 9,
        };

        // 4. osh (当前 shell)
        infos[3] = ProcessInfo {
            pid: 12,
            ppid: 1,
            state: 1, // Running
            name: {
                let mut n = [0u8; 16];
                n[..3].copy_from_slice(b"osh");
                n
            },
            name_len: 3,
        };

        Ok(4)
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
