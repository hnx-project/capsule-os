use crate::task::thread::{Priority, Thread, ThreadState};
use crate::task::switch_to;
use crate::arch::trap::{disable_irqs, enable_irqs};
use core::sync::atomic::{AtomicBool, Ordering};

pub const MAX_THREADS: usize = 16;
pub const PRIORITY_LEVELS: usize = 5;

struct ThreadQueue {
    items: [Option<usize>; MAX_THREADS],
    count: usize,
}

impl ThreadQueue {
    const fn new() -> Self {
        ThreadQueue {
            items: [None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None],
            count: 0,
        }
    }

    fn push(&mut self, thread_id: usize) -> bool {
        if self.count >= MAX_THREADS {
            return false;
        }
        self.items[self.count] = Some(thread_id);
        self.count += 1;
        true
    }

    fn pop_highest_priority(&mut self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let result = self.items[0];
        for i in 1..self.count {
            self.items[i - 1] = self.items[i];
        }
        self.count -= 1;
        self.items[self.count] = None;
        result
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

pub struct Scheduler {
    threads: [Option<Thread>; MAX_THREADS],
    queues: [ThreadQueue; PRIORITY_LEVELS],
    current_idx: Option<usize>,
    tick_count: usize,
    running: bool,
}

static SCHEDULER_LOCK: AtomicBool = AtomicBool::new(false);

pub static mut SCHEDULER: Scheduler = Scheduler::new();

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            threads: [None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None],
            queues: [ThreadQueue::new(), ThreadQueue::new(), ThreadQueue::new(), ThreadQueue::new(), ThreadQueue::new()],
            current_idx: None,
            tick_count: 0,
            running: false,
        }
    }

    fn lock(&self) {
        unsafe {
            disable_irqs();
        }
        while SCHEDULER_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn unlock(&self) {
        SCHEDULER_LOCK.store(false, Ordering::Release);
        unsafe {
            enable_irqs();
        }
    }

    fn priority_to_index(priority: Priority) -> usize {
        priority as usize
    }

    fn find_empty_slot(&self) -> Option<usize> {
        for i in 0..MAX_THREADS {
            if self.threads[i].is_none() {
                return Some(i);
            }
        }
        None
    }

    pub fn add(&mut self, mut thread: Thread) {
        self.lock();

        let slot = self.find_empty_slot();
        if let Some(idx) = slot {
            
            let priority_idx = Self::priority_to_index(thread.priority);
            thread.state = ThreadState::Ready;
            self.threads[idx] = Some(thread);

            if !self.queues[priority_idx].push(idx) {
                self.threads[idx] = None;
                panic!("[SCHED] Queue overflow!");
            } else {
                crate::log_info!("SCHED", "ADDED");
            }
        } else {
            panic!("[SCHED] Max thread count exceeded!");
        }

        self.unlock();
    }

    fn pop_next_from_all_queues(&mut self) -> Option<usize> {
        for i in 0..PRIORITY_LEVELS {
            if let Some(idx) = self.queues[i].pop_highest_priority() {
                return Some(idx);
            }
        }
        None
    }

    fn requeue_current(&mut self, idx: usize) {
        let priority_idx = if let Some(ref t) = self.threads[idx] {
            Self::priority_to_index(t.priority)
        } else {
            return;
        };
        self.queues[priority_idx].push(idx);
    }

    pub fn run(&mut self) -> ! {
        self.lock();
        self.running = true;

        if let Some(idx) = self.pop_next_from_all_queues() {
            self.current_idx = Some(idx);
            unsafe {
                if let Some(ref mut t) = self.threads[idx] {
                    t.state = ThreadState::Running;
                    t.reset_time_slice();
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                use crate::mm::mmu::ArchMmu;
                crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();
            }

            let mut dummy_ctx = crate::task::thread::ThreadContext::default();
            self.unlock();
            unsafe {
                switch_to(&mut dummy_ctx, &mut self.threads[idx].as_mut().unwrap().context);
            }
        }

        panic!("[SCHED] No threads to run!");
    }

    pub fn schedule(&mut self) {
        self.lock();

        if !self.running {
            self.unlock();
            return;
        }

        self.tick_count = self.tick_count.wrapping_add(1);

        let prev_idx = match self.current_idx {
            Some(idx) => idx,
            None => {
                self.unlock();
                return;
            }
        };

        let prev_state = if let Some(ref mut t) = self.threads[prev_idx] {
            if t.state == ThreadState::Running {
                t.state = ThreadState::Ready;
            }
            t.remaining_ticks = t.remaining_ticks.saturating_sub(1);
            if t.remaining_ticks == 0 {
                t.decay_priority();
            }
            t.state
        } else {
            ThreadState::Dead
        };

        if prev_state == ThreadState::Ready {
            self.requeue_current(prev_idx);
        }

        if let Some(next_idx) = self.pop_next_from_all_queues() {
            let prev_name = if let Some(ref t) = self.threads[prev_idx] { t.name } else { "none" };
            let next_name = if let Some(ref t) = self.threads[next_idx] { t.name } else { "none" };

            if let Some(ref mut t) = self.threads[next_idx] {
                t.state = ThreadState::Running;
                t.reset_time_slice();
            }

            print_switch(prev_name, next_name);
            self.current_idx = Some(next_idx);

            // **Short-circuit when there's nothing to switch to.**  When a
            // timer IRQ nests inside kernel EL1 (e.g. during
            // `sys_spawn`'s safe-copy loops), `schedule()` runs with
            // `current_idx == loader` and the only Ready thread is
            // `loader` itself, so `pop_next` returns `loader` again.
            // Calling `switch_to(loader, loader)` would still execute
            // its trailing `ret x30 = user_eret_stub → eret`, which
            // **hijacks** the calling kernel stack frame: control flow
            // jumps to EL0 as if we'd finished a context switch, the
            // syscall handler's epilogue (`msr elr_el1; eret`) never
            // runs, and the original `syscalls::spawn("devmgr", ...)`
            // SVC never returns.  That cascades into the user's
            // `tp!("T11: returned from spawn(devmgr)")` never firing
            // because the loader's PC is stuck at the SVC+4 PC inside
            // an unbroken timer-tick → switch_to → eret → SVC trap →
            // timer IRQ → switch_to → eret loop.
            //
            // Skip the `switch_to` + `ret` entirely when we're already
            // on the thread we'd be switching to.  The time-slice
            // accounting (`t.reset_time_slice` above) and state
            // transition (Ready → Running) have already been applied
            // in this same scope, so control returning to the caller
            // of `schedule()` is the correct semantic: we stay on the
            // current kernel stack frame and resume the interrupted
            // syscall / IRQ handler's epilogue.
            if prev_idx == next_idx {
                crate::log_info!("SCHED-SAME", "prev_idx == next_idx == {} (skip switch_to hijack)", prev_idx);
                self.unlock();
                return;
            }

            let prev_context_ptr = &mut self.threads[prev_idx].as_mut().unwrap().context as *mut _;
            let next_context_ptr = &mut self.threads[next_idx].as_mut().unwrap().context as *mut _;

            #[cfg(target_arch = "aarch64")]
            {
                use crate::mm::mmu::ArchMmu;
                crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();
            }

            self.unlock();

            unsafe {
                switch_to(&mut *prev_context_ptr, &*next_context_ptr);
            }
        } else {
            let all_dead = self.threads.iter().all(|t| match t {
                None => true,
                Some(thread) => thread.state == ThreadState::Dead,
            });

            self.unlock();

            if all_dead {
                crate::log_error!("SCHED", "No runnable threads left! Halting CPU safely...");
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

            self.current_idx = None;
        }
    }

    pub fn tick(&mut self) {
        self.schedule();
    }

    pub fn current_thread_name(&self) -> &'static str {
        self.lock();
        let name = if let Some(idx) = self.current_idx {
            if let Some(ref t) = self.threads[idx] {
                t.name
            } else {
                "none"
            }
        } else {
            "none"
        };
        self.unlock();
        name
    }

    pub fn get_current_thread_ptr(&mut self) -> Option<*mut Thread> {
        self.lock();
        let ptr = self.current_idx.and_then(|idx| self.threads[idx].as_mut().map(|t| t as *mut Thread));
        self.unlock();
        ptr
    }

    pub fn get_thread_ptr(&mut self, thread_id: usize) -> Option<*mut Thread> {
        self.lock();
        let mut ptr = None;
        for i in 0..MAX_THREADS {
            if let Some(ref mut t) = self.threads[i] {
                if t.id == thread_id {
                    ptr = Some(t as *mut Thread);
                    break;
                }
            }
        }
        self.unlock();
        ptr
    }

    pub fn wake_thread(&mut self, thread_id: usize) {
        self.lock();
        for i in 0..MAX_THREADS {
            if let Some(ref mut t) = self.threads[i] {
                if t.id == thread_id && (t.state == ThreadState::Blocked || t.state == ThreadState::Sleeping) {
                    t.state = ThreadState::Ready;
                    self.requeue_current(i);
                    break;
                }
            }
        }
        self.unlock();
    }
}

fn print_switch(from: &str, to: &str) {
    crate::log_debug!("SCHED", "switch: {} -> {}", from, to);
}
