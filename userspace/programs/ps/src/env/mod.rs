#[derive(Debug, Clone, Copy)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub state: u8,      // 0 = Unknown, 1 = Running, 2 = Sleeping, 3 = Zombie
    pub name: [u8; 16], // 固定 16 字节进程名缓冲 (no_std 友好)
    pub name_len: usize,
}

impl ProcessInfo {
    pub const fn new() -> Self {
        Self {
            pid: 0,
            ppid: 0,
            state: 0,
            name: [0u8; 16],
            name_len: 0,
        }
    }
}

pub trait ProcSystem {
    /// 获取当前系统的进程快照，返回写入到 infos 数组的实际数量
    fn get_process_list(&self, infos: &mut [ProcessInfo]) -> Result<usize, ()>;
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
