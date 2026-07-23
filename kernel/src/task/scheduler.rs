use crate::arch::{ArchHardware, CurrentArch};
use crate::arch::trap::{disable_irqs, enable_irqs};
use crate::smp::{self, MAX_CORES};
use crate::task::thread::{Priority, Thread, ThreadState};
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
            items: [None; MAX_THREADS],
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

/// SMP-aware scheduler.
///
/// `current_indices[c]` is the per-core currently-running thread
/// slot, indexed by physical CPU slot.  When a thread is in
/// `state == Running`, its `owner_core` matches the slot it is
/// running on; this pair of fields forms the per-iteration occupancy
/// invariant.
pub struct Scheduler {
    threads: [Option<Thread>; MAX_THREADS],
    queues: [ThreadQueue; PRIORITY_LEVELS],
    current_indices: [Option<usize>; MAX_CORES],
    running: bool,
}

static SCHEDULER_LOCK: AtomicBool = AtomicBool::new(false);

pub static mut SCHEDULER: Scheduler = Scheduler::new();

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            threads: [const { None }; MAX_THREADS],
            queues: [const { ThreadQueue::new() }; PRIORITY_LEVELS],
            current_indices: [const { None }; MAX_CORES],
            running: false,
        }
    }

    /// Acquire the scheduler lock and disable IRQs on this CPU.  The
    /// returned flags value must be passed to `unlock()` to restore
    /// the IRQ state.  Callers must not hold any other lock at the
    /// same time.
    pub fn lock(&self) -> usize {
        let flags = unsafe { CurrentArch::local_irq_save() };
        while SCHEDULER_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        flags
    }

    pub fn unlock(&self, flags: usize) {
        SCHEDULER_LOCK.store(false, Ordering::Release);
        unsafe {
            CurrentArch::local_irq_restore(flags);
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
        let flags = self.lock();

        let slot = self.find_empty_slot();
        if let Some(idx) = slot {
            let priority_idx = Self::priority_to_index(thread.priority);
            thread.state = ThreadState::Ready;
            thread.owner_core = None;
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

        self.unlock(flags);
    }

    /// Pop the next thread the given core should run.  The candidate
    /// must already satisfy `state == Ready && owner_core == None`;
    /// if it doesn't, we drop it on the floor and keep scanning.
    /// Called with the scheduler lock held.
    ///
    /// The function commits the ownership transition atomically:
    /// `threads[idx].owner_core` is set to `Some(my)` before the
    /// slot is returned, so a concurrent schedulder cannot pick the
    /// same thread even if it acquires the lock immediately after
    /// we release it.
    fn pop_next_for_core(&mut self, my: usize) -> Option<usize> {
        // Bound the scan so a pathological state machine doesn't
        // loop indefinitely.
        for _ in 0..MAX_THREADS {
            let candidate = (0..PRIORITY_LEVELS)
                .find_map(|p| self.queues[p].pop_highest_priority());
            match candidate {
                None => {
                    // Ready queues are empty (or every queued
                    // thread we examined was Dead and was therefore
                    // already dropped).
                    return None;
                }
                Some(idx) => {
                    let accept = match self.threads[idx].as_ref() {
                        Some(t) => {
                            t.state == ThreadState::Ready && t.owner_core.is_none()
                        }
                        None => false,
                    };
                    if accept {
                        let t_mut = self.threads[idx].as_mut().expect("just checked");
                        t_mut.owner_core = Some(my);
                        return Some(idx);
                    }
                    // Drop the unacceptable candidate and try the
                    // next one.
                }
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

    /// Apply the page table / ASID associated with `idx`.  On
    /// aarch64 this is a single `MSR ttbr0_el1, ...` plus a TLB
    /// flush; no-op for architectures without `ttbr0`.
    fn apply_ttbr_for(&self, idx: usize) {
        if let Some(t) = self.threads[idx].as_ref() {
            let pid = t.process_id;
            #[cfg(target_arch = "aarch64")]
            if let Some((l0_pa, asid)) =
                crate::task::process::find_process_l0_user_pa(pid)
            {
                crate::arch::aarch64::mmu::set_ttbr0_el1(l0_pa, asid);
            }
        }
    }

    /// First-time entry: pick the highest-priority thread on this
    /// core, mark it running, and context-switch out of the boot
    /// stack.  Never returns.
    pub fn run(&mut self) -> ! {
        let flags = self.lock();
        self.running = true;
        let my = smp::current_core_id();
        if my >= MAX_CORES {
            // Offlined slot — can't happen for core 0, fall back to
            // the legacy "find any thread" path but be defensive.
            self.unlock(flags);
            panic!("[SCHED] run() called from an unregistered slot");
        }
        let idx = self
            .pop_next_for_core(my)
            .expect("[SCHED] No threads to run at boot");

        if let Some(t) = self.threads[idx].as_mut() {
            t.state = ThreadState::Running;
            t.reset_time_slice();
        }
        self.current_indices[my] = Some(idx);

        self.apply_ttbr_for(idx);
        self.unlock(flags);

        // Switch out to the selected thread using a dummy
        // `prev` context.  When the next kernel entry happens
        // (timer IRQ, exception), the scheduler will eventually
        // swap contexts the normal way.
        let mut dummy = crate::task::thread::ThreadContext::default();
        let next_ptr =
            &self.threads[idx].as_ref().unwrap().context as *const _ as *mut _;
        unsafe {
            CurrentArch::switch_context(&mut dummy, next_ptr);
        }
        // switch_to never returns when called with a dummy prev.
        loop {
            unsafe { CurrentArch::wait_for_interrupt() };
        }
    }

    pub fn schedule(&mut self) {
        let flags = self.lock();
        let my = smp::current_core_id();
        if my >= MAX_CORES {
            self.unlock(flags);
            return;
        }

        if !self.running {
            self.unlock(flags);
            return;
        }

        // Wake up any sleeping threads whose timer has elapsed.
        self.check_sleeping_threads(crate::drivers::timer::get_ticks());

        // 1. Release this core's currently-running thread (if any).
        let prev_idx = match self.current_indices[my] {
            Some(i) => {
                // Compute everything in one &mut borrow scope, then
                // requeue separately so we don't double-borrow
                // `self.threads` via `requeue_current`.
                let needs_requeue = {
                    let t = self.threads[i].as_mut().expect("current thread vanished");
                    let was_running = t.state == ThreadState::Running;
                    if was_running {
                        t.state = ThreadState::Ready;
                    }
                    t.remaining_ticks = t.remaining_ticks.saturating_sub(1);
                    if t.remaining_ticks == 0 {
                        t.decay_priority();
                    }
                    t.owner_core = None;
                    was_running
                };
                if needs_requeue {
                    self.requeue_current(i);
                }
                i
            }
            None => usize::MAX, // first time on this core or returning from WFE
        };

        // 2. Pick the next thread.  Strictly
        //    `Ready && owner_core == None`.
        let next_idx = match self.pop_next_for_core(my) {
            Some(i) => i,
            None => {
                // Nothing to run on this core.  Park in WFE —
                // another core may wake us by `wake_thread` setting
                // ONLINE_MASK[my], or by an IRQ from outside.
                self.current_indices[my] = None;
                self.unlock(flags);
                unsafe { CurrentArch::wait_for_event(); }
                // After wake, reschedule immediately.
                return self.schedule();
            }
        };

        // 3. State submit for `next`.
        {
            let t = self.threads[next_idx].as_mut().unwrap();
            t.state = ThreadState::Running;
            t.reset_time_slice();
        }
        self.current_indices[my] = Some(next_idx);

        // 4. prev == next short circuit (IRQ nesting or a yield
        //    that didn't actually move us off this thread).
        if prev_idx == next_idx {
            // Make sure TTBR0 is in sync (cheap) and bail out
            // without an actual register switch.
            self.apply_ttbr_for(next_idx);
            self.unlock(flags);
            return;
        }

        // 5. Apply TTBR0 + TLB flush for the new thread BEFORE we
        //    release the lock; the actual `switch_context` is then
        //    lockless, satisfying the "decoupled locking &
        //    blocking" requirement.
        self.apply_ttbr_for(next_idx);
        self.unlock(flags);

        // 6. Register-level context switch.
        if prev_idx == usize::MAX {
            // Secondary-core boot: no prev, just push a dummy
            // context and jump to `next`.
            let mut dummy = crate::task::thread::ThreadContext::default();
            let next_ptr =
                &self.threads[next_idx].as_ref().unwrap().context as *const _ as *mut _;
            unsafe {
                CurrentArch::switch_context(&mut dummy, next_ptr);
            }
            // Should not return on this path.
            return;
        }

        // Two-element borrow conflicts with `&mut self`; switch
        // contexts through raw pointers instead.  Safe because
        // `prev_idx` and `next_idx` are distinct and the scheduler
        // lock prevents either index from being mutated by other
        // CPUs between obtaining the pointers and the actual
        // switch.
        let prev_ptr =
            &mut self.threads[prev_idx].as_mut().unwrap().context as *mut _;
        let next_ptr = &self.threads[next_idx].as_ref().unwrap().context as *const _;
        unsafe {
            CurrentArch::switch_context(prev_ptr, next_ptr);
        }

        // When we return here, `prev` is once again the
        // currently-running thread on `my`.  Reapply the page
        // table so the EL1 code that follows sees the user L0
        // set up for our own process.
        self.apply_ttbr_for(prev_idx);
    }

    pub fn get_current_thread_ptr(&mut self) -> Option<*mut Thread> {
        let flags = self.lock();
        let my = smp::current_core_id();
        let slot = if my < MAX_CORES {
            self.current_indices[my]
        } else {
            None
        };
        let ptr = slot.and_then(|idx| self.threads[idx].as_mut().map(|t| t as *mut Thread));
        self.unlock(flags);
        ptr
    }

    pub fn get_thread_ptr(&mut self, thread_id: usize) -> Option<*mut Thread> {
        let flags = self.lock();
        let mut ptr = None;
        for i in 0..MAX_THREADS {
            if let Some(ref mut t) = self.threads[i] {
                if t.id == thread_id {
                    ptr = Some(t as *mut Thread);
                    break;
                }
            }
        }
        self.unlock(flags);
        ptr
    }

    pub fn wake_thread(&mut self, thread_id: usize) {
        let flags = self.lock();
        for i in 0..MAX_THREADS {
            if let Some(ref mut t) = self.threads[i] {
                if t.id == thread_id
                    && (t.state == ThreadState::Blocked
                        || t.state == ThreadState::Sleeping)
                {
                    if t.owner_core.is_some() {
                        // Defensive — shouldn't happen if everyone
                        // releases ownership before blocking, but
                        // if it does, silently drop the claim.
                        t.owner_core = None;
                    }
                    t.state = ThreadState::Ready;
                    self.requeue_current(i);
                    break;
                }
            }
        }
        self.unlock(flags);
    }

    /// Scan all threads in the system. Any thread in `ThreadState::Sleeping` state whose
    /// sleep time has elapsed is moved back to the `ThreadState::Ready` state.
    ///
    /// This is called within the preemptive scheduler while the scheduler lock is held.
    pub fn check_sleeping_threads(&mut self, current_ticks: u64) {
        for i in 0..MAX_THREADS {
            if let Some(ref mut t) = self.threads[i] {
                if t.state == ThreadState::Sleeping {
                    if let Some(wakeup_tick) = t.sleep_until {
                        if current_ticks >= wakeup_tick {
                            t.state = ThreadState::Ready;
                            t.owner_core = None;
                            t.sleep_until = None;
                            let priority_idx = Self::priority_to_index(t.priority);
                            if !self.queues[priority_idx].push(i) {
                                crate::log_warn!("SCHED", "check_sleeping_threads: Failed to requeue sleeping thread!");
                            }
                        }
                    }
                }
            }
        }
    }
}

fn print_switch(from: &str, to: &str) {
    crate::log_debug!("SCHED", "switch: {} -> {}", from, to);
}
