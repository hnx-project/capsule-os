use core::sync::atomic::{AtomicUsize, Ordering};

static THREAD_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug)]
pub struct Thread {
    pub id: usize,
    pub name: &'static str,
    pub entry: usize,
    pub stack_ptr: usize,
    pub state: ThreadState,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Initial, Ready, Running, Blocked, Sleeping, Dead,
}

impl Thread {
    pub fn new_kernel(name: &'static str, entry: extern "C" fn()) -> Self {
        Thread {
            id: THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name, entry: entry as usize, stack_ptr: 0,
            state: ThreadState::Ready, priority: 128,
        }
    }
}
