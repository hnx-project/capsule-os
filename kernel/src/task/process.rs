use shared::status::Status;

#[derive(Debug)]
pub struct Process {
    pub id: u64,
    pub name: &'static str,
    pub state: ProcessState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Initial, Running, Zombie, Dead,
}

impl Process {
    pub fn new(name: &'static str) -> Status {
        let _ = name;
        Status::Ok
    }
}
