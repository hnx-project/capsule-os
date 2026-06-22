use crate::task::thread::Thread;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};

pub struct Scheduler {
    current: Option<Thread>,
    tick_count: AtomicUsize,
    running: AtomicBool,
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler { current: None, tick_count: AtomicUsize::new(0), running: AtomicBool::new(true) }
    }
    pub fn add(&self, _thread: Thread) {}
    pub fn run(&self) -> ! {
        self.running.store(true, Ordering::SeqCst);
        loop {
            self.tick_count.fetch_add(1, Ordering::SeqCst);
        }
    }
}
