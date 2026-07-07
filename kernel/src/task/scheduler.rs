use crate::task::thread::{Thread, ThreadState};
use crate::task::switch_to;

pub const MAX_THREADS: usize = 16;

pub struct Scheduler {
    threads: [Option<Thread>; MAX_THREADS],
    current: Option<usize>,
    tick_count: usize,
    running: bool,
}

pub static mut SCHEDULER: Scheduler = Scheduler::new();

impl Scheduler {
    pub const fn new() -> Self {
        const NONE_THREAD: Option<Thread> = None;
        Scheduler {
            threads: [NONE_THREAD; MAX_THREADS],
            current: None,
            tick_count: 0,
            running: false,
        }
    }

    pub fn add(&mut self, thread: Thread) {
        for slot in self.threads.iter_mut() {
            if slot.is_none() {
                *slot = Some(thread);
                return;
            }
        }
        panic!("[SCHED] Max thread count exceeded!");
    }

    pub fn run(&mut self) -> ! {
        self.running = true;
        let next_idx = 0;
        self.current = Some(next_idx);

        let next_thread = self.threads[next_idx].as_mut().expect("no threads in scheduler");
        next_thread.state = ThreadState::Running;

        let mut dummy_ctx = crate::task::thread::ThreadContext::default();
        unsafe {
            switch_to(&mut dummy_ctx, &next_thread.context);
        }

        unreachable!("Scheduler::run returned");
    }

    pub fn schedule(&mut self) {
        if !self.running {
            return;
        }

        self.tick_count = self.tick_count.wrapping_add(1);

        let current_idx = match self.current {
            Some(idx) => idx,
            None => return,
        };

        // Find the next runnable thread (starting from (current_idx + 1) % MAX_THREADS)
        let mut next_idx = None;
        for i in 1..=MAX_THREADS {
            let idx = (current_idx + i) % MAX_THREADS;
            if let Some(t) = &self.threads[idx] {
                if t.state == ThreadState::Ready || t.state == ThreadState::Initial {
                    next_idx = Some(idx);
                    break;
                }
            }
        }

        if let Some(n_idx) = next_idx {
            if n_idx == current_idx {
                // Only current thread is ready, continue running it
                return;
            }

            // Switch to the next thread
            self.current = Some(n_idx);

            unsafe {
                let threads_ptr = self.threads.as_mut_ptr();
                let cur_thread = &mut *threads_ptr.add(current_idx);
                let next_thread = &mut *threads_ptr.add(n_idx);

                if let (Some(cur), Some(next)) = (cur_thread, next_thread) {
                    if cur.state == ThreadState::Running {
                        cur.state = ThreadState::Ready;
                    }
                    next.state = ThreadState::Running;

                    // Log context switch on every switch for visual confirmation
                    print_switch(cur.name, next.name);

                    switch_to(&mut cur.context, &next.context);
                }
            }
        } else {
            // No runnable threads found.
            // If the current thread is DEAD, we must halt the CPU to prevent infinite exception/ERET loops!
            let current_dead = if let Some(cur) = self.get_current_thread_mut() {
                cur.state == ThreadState::Dead
            } else {
                true
            };

            if current_dead {
                crate::log_error!("SCHED", "No runnable threads left and current thread is DEAD! Halting CPU safely...");
                unsafe {
                    crate::arch::trap::disable_irqs();
                    loop {
                        #[cfg(target_arch = "aarch64")]
                        core::arch::asm!("wfe");
                        #[cfg(target_arch = "riscv64")]
                        core::arch::asm!("wfi");
                    }
                }
            }
        }
    }

    pub fn current_thread_name(&self) -> &'static str {
        if let Some(idx) = self.current {
            if let Some(t) = &self.threads[idx] {
                return t.name;
            }
        }
        "none"
    }

    pub fn get_current_thread_mut(&mut self) -> Option<&mut Thread> {
        if let Some(idx) = self.current {
            self.threads[idx].as_mut()
        } else {
            None
        }
    }

    pub fn get_thread_mut(&mut self, thread_id: usize) -> Option<&mut Thread> {
        for slot in self.threads.iter_mut() {
            if let Some(t) = slot {
                if t.id == thread_id {
                    return Some(t);
                }
            }
        }
        None
    }
}

fn print_switch(from: &str, to: &str) {
    crate::log_debug!("SCHED", "switch: {} -> {}", from, to);
}
