use shared::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    OneShot,
    Periodic,
    Continuous,
}

pub trait Timer: Send + Sync {
    fn new() -> Status;
    fn set_timeout(&mut self, ticks: u64, mode: TimerMode) -> Status;
    fn cancel(&mut self) -> Status;
    fn get_ticks(&self) -> u64;
    fn ticks_per_second() -> u64;
}
