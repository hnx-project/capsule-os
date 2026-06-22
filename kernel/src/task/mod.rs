pub mod scheduler;
pub mod thread;
pub mod process;

pub use scheduler::Scheduler;
pub use thread::Thread;
pub use process::Process;

pub fn create_init_thread() -> Thread {
    Thread::new_kernel("init", init_entry)
}

extern "C" fn init_entry() {
    loop {}
}
