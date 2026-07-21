use crate::task::thread::{Priority, Thread, ThreadState};
use crate::arch::{ArchHardware, CurrentArch};
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
    running: bool,
    /// H5: DAIF mask saved by `lock()`, restored by `unlock()`.
    /// Stored as a bare `core::cell::Cell<usize>` to allow interior mutability without undefined casting.
    daif_save: core::cell::Cell<usize>,
}

static SCHEDULER_LOCK: AtomicBool = AtomicBool::new(false);

pub static mut SCHEDULER: Scheduler = Scheduler::new();

static mut SCHED_SAME_HIT_COUNT: u32 = 0;

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            threads: [const { None }; MAX_THREADS],
            queues: [const { ThreadQueue::new() }; PRIORITY_LEVELS],
            current_idx: None,
            running: false,
            daif_save: core::cell::Cell::new(0),
        }
    }

    fn lock(&self) {
        // H5 (KERNEL_HEALTH): save the current DAIF mask and only
        // re-enable IRQs on unlock if they were enabled at the
        // matching `lock()`.  The previous implementation
        // unconditionally called `enable_irqs()` on unlock, which
        // races IRQ nesting and SSP-on contention.  AArch64 has no
        // dedicated IRQ-on-PUSH, so the saved-and-restored pair is
        // the PSTATE-safe equivalent of Linux's
        // `local_irq_save` / `local_irq_restore`.
        unsafe {
            let flags = crate::arch::CurrentArch::local_irq_save();
            self.daif_save.set(flags);
        }
        while SCHEDULER_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn unlock(&self) {
        SCHEDULER_LOCK.store(false, Ordering::Release);
        unsafe {
            let flags = self.daif_save.get();
            crate::arch::CurrentArch::local_irq_restore(flags);
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

    /// Pop the highest-priority ready thread, skipping any thread that
    /// has been marked `ThreadState::Dead` since it was last enqueued.
    ///
    /// **Why we skip Dead here**: an EL0 fault path (`aarch64_sync_el0_handler`
    /// and the new `aarch64_serror_el0_handler`) marks the current
    /// user thread `Dead` *and then* calls `SCHEDULER.schedule()`.  At
    /// that point the dying thread is still sitting in some priority
    /// queue (it was Running a moment ago, got requeued on the
    /// previous timer tick, and never re-popped because the fault
    /// stole the timer).  Without the Dead filter, `pop_next` would
    /// return the dead thread itself, `prev_idx == next_idx` would
    /// short-circuit, and the system would loop forever in
    /// `SCHED-SAME`.  With the filter, we re-scan up to `MAX_THREADS`
    /// candidates; if every queued thread is Dead, fall through to
    /// a linear scan of `self.threads` to find a still-Running/Ready
    /// thread (e.g. devmgr, which lives outside the ready queues
    /// while it's in EL0).  This is O(N) per call but N is tiny
    /// (MAX_THREADS = 16) and the only alternatives — pruning dead
    /// entries from every queue on every kill, or maintaining a
    /// separate "alive" bitmap — are far more invasive.
    fn pop_next_from_all_queues(&mut self) -> Option<usize> {
        for _ in 0..MAX_THREADS {
            let candidate = (0..PRIORITY_LEVELS)
                .find_map(|i| self.queues[i].pop_highest_priority());
            match candidate {
                None => {
                    // Ready queues are empty (or every queued thread
                    // we examined was Dead and was therefore already
                    // dropped).  Look for any thread that is *not*
                    // Dead — typically one stuck in Running because
                    // it wasn't requeued when its time slice expired
                    // (e.g. the `loader` was killed before its next
                    // tick, but `devmgr` is still in EL0).
                    return self
                        .threads
                        .iter()
                        .enumerate()
                        .find_map(|(i, t)| match t {
                            Some(thread) if thread.state != ThreadState::Dead => Some(i),
                            _ => None,
                        });
                }
                Some(idx) => {
                    let is_dead = self.threads[idx]
                        .as_ref()
                        .map(|t| t.state == ThreadState::Dead)
                        .unwrap_or(true);
                    if !is_dead {
                        return Some(idx);
                    }
                    // Drop the dead candidate and try the next one.
                }
            }
        }
        // Every ready-queue entry across every priority was Dead, and
        // the linear scan of `self.threads` found nothing alive
        // either.  Caller will hit the "all dead" branch.
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
            unsafe {
                crate::arch::CurrentArch::flush_tlb();
            }

            let mut dummy_ctx = crate::task::thread::ThreadContext::default();
            self.unlock();
            unsafe {
                CurrentArch::switch_context(&mut dummy_ctx, &mut self.threads[idx].as_mut().unwrap().context);
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

        let current_ticks = crate::drivers::timer::get_ticks();
        self.check_sleeping_threads(current_ticks);

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

            self.current_idx = Some(next_idx);

            // The SCHED-SAME short-circuit only applies when `prev`
            // was a live Running thread that the pop_next logic
            // happened to re-select (e.g. the only ready thread is
            // the same one we just ran).  If the caller killed the
            // prev thread before invoking `schedule()` (the EL0
            // fault / SError path), `prev_state` is already `Dead`
            // and we *must* perform the switch so the dispatcher
            // can `eret` into a non-dead context.

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
            //
            // **Phase 3 hardening (prev==next return path)**: even
            // when we skip the actual `switch_to`, the previous
            // (pre-fix) implementation never reloaded `TTBR0_EL1`.
            // That left the CPU's translation regime pointed at
            // whatever process last ran `set_ttbr0_el1` -- typically
            // a now-dead child like `ls`.  When the parent (`osh`)
            // then resumed on this same kernel stack, the very next
            // user-side instruction fetch could Translation-fault on
            // a perfectly valid VA in `osh`'s own L0.  We now reload
            // `TTBR0_EL1` from the *current* process's L0 + ASID on
            // every resumption, regardless of whether we hit the
            // short-circuit or the full switch path.  This is the
            // explicit fix for the KERNEL_HEALTH A2 follow-on
            // (EL0-FAULT EC=0x24 after `ls` exit).
            if prev_idx == next_idx && prev_state != ThreadState::Dead {
                // When the scheduler selects the same thread (common
                // during IRQ nesting), reload TTBR0_EL1 + flush the
                // icache at the thread's user PC so the upcoming eret
                // uses the correct translation regime.  This is the
                // same pair of operations the full switch path does
                // before/after switch_to, but without the switch itself.
                #[cfg(target_arch = "aarch64")]
                {
                    let cur_pid = self.threads[prev_idx].as_ref().unwrap().process_id;
                    if let Some((cur_l0_pa, cur_asid)) =
                        crate::task::process::find_process_l0_user_pa(cur_pid)
                    {
                        crate::arch::aarch64::mmu::set_ttbr0_el1(cur_l0_pa, cur_asid);
                    }
                    if let Some(ref t) = self.threads[prev_idx] {
                        let user_pc = t.context.elr;
                        if user_pc != 0 {
                            unsafe {
                                <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::instruction_barrier();
                            }
                        }
                        // PRE-ERET generation-stamp check: if the thread's
                        // PageTableTree generation doesn't match the process,
                        // the thread holds a stale tree handle.
                        if let Some(gen) =
                            crate::task::process::find_process_page_table_gen(t.process_id)
                        {
                            if t.context.page_table_gen != gen {
                                crate::log_error!(
                                    "SCHED",
                                    "PRE-ERET: pid={} page_table_gen mismatch: thread={} process={}",
                                    t.process_id, t.context.page_table_gen, gen
                                );
                            }
                        }
                        // PRE-ERET validation: walk the page table for
                        // user_sp (checking the page below SP, since SP
                        // can start at stack_top which is one-past-end).
                        let usp = t.context.user_sp as usize;
                        let check_va = if usp & 0xFFF == 0 && usp >= 8 {
                            usp - 8
                        } else {
                            usp
                        };
                        if let Some((l0_pa, _)) =
                            crate::task::process::find_process_l0_user_pa(t.process_id)
                        {
                    match crate::arch::aarch64::mmu::translate_user_va(l0_pa, check_va) {
                        Some(pa) => {
                            crate::log_debug!("SCHED", "PRE-ERET: user_sp={:#x} -> PA {:#x} pid={} OK",
                                usp, pa, t.process_id);
                        }
                        None => {
                            crate::log_error!(
                                "SCHED",
                                "PRE-ERET: user_sp={:#x} check={:#x} pid={} has no valid PTE!",
                                usp, check_va, t.process_id
                            );
                            crate::log_error!(
                                "SCHED",
                                "  PT-VALIDATE pid={}: {}",
                                t.process_id,
                                if crate::task::process::validate_process_page_table(t.process_id)
                                    { "tree OK" } else { "** TREE CORRUPTED **" }
                            );
                    }
                }
                }
                }
                }
                self.unlock();
                // Use a volatile write to ensure the compiler doesn't elide
                // our early return: the dummy `static mut` sink prevents the
                // LLVM optimizer from realizing that we just fall through
                // into the switch_to block, and the `core::hint::black_box`
                // hints that the comparison has side effects that matter.
                unsafe {
                    core::ptr::write_volatile(&mut SCHED_SAME_HIT_COUNT as *mut u32, 1);
                }
                core::hint::black_box(prev_idx);
                return;
            }

            let prev_context_ptr = &mut self.threads[prev_idx].as_mut().unwrap().context as *mut _;
            let next_context_ptr = &mut self.threads[next_idx].as_mut().unwrap().context as *mut _;

            // CRITICAL: switch TTBR0_EL1 to the *next* thread's process
            // page table before we context-switch.  Without this, the
            // current TTBR0 still points at whichever process most
            // recently called `Process::launch_user_program` — and
            // every other process's user VA range would walk into the
            // wrong L0 page, faulting on a perfectly valid address.
            //
            // We also have to swap back to the *previous* thread's L0
            // on the way back, because the new process's L0 only
            // covers that new process's user VA; the kernel still
            // needs to find the old user stack during `eret`/signal
            // teardown.  Doing both in one place (this function) keeps
            // the policy in one spot.
            //
            // **Phase 3 (ASID)**: the L0 PA now travels with its
            // owning process's 8-bit ASID.  `set_ttbr0_el1` packs
            // both into a single MSR, and the inner barrier
            // sequence (`tlbi vmalle1is` + `dsb ish` + `isb`)
            // ensures the new translation regime is observable to
            // the page-table walker before the first user-side
            // fetch after `eret`.
            let next_pid = self.threads[next_idx].as_ref().unwrap().process_id;
            let prev_pid = self.threads[prev_idx].as_ref().unwrap().process_id;

            let next_l0_asid = crate::task::process::find_process_l0_user_pa(next_pid);
            #[cfg(target_arch = "aarch64")]
            if let Some((next_l0_pa, next_asid)) = next_l0_asid {
                crate::arch::aarch64::mmu::set_ttbr0_el1(next_l0_pa, next_asid);
            }
            #[cfg(target_arch = "aarch64")]
            unsafe {
                crate::arch::CurrentArch::flush_tlb();
            }

            #[cfg(target_arch = "aarch64")]
            if let Some(ref t) = self.threads[next_idx] {
                if let Some(gen) =
                    crate::task::process::find_process_page_table_gen(t.process_id)
                {
                    if t.context.page_table_gen != gen {
                        crate::log_error!(
                            "SCHED",
                            "PRE-ERET: pid={} page_table_gen mismatch: thread={} process={}",
                            t.process_id, t.context.page_table_gen, gen
                        );
                    }
                }
                let usp = t.context.user_sp as usize;
                let check_va = if usp & 0xFFF == 0 && usp >= 8 {
                    usp - 8
                } else {
                    usp
                };
                if let Some((l0_pa, _)) =
                    crate::task::process::find_process_l0_user_pa(t.process_id)
                {
                    match crate::arch::aarch64::mmu::translate_user_va(l0_pa, check_va) {
                        Some(pa) => {
                            crate::log_debug!("SCHED", "PRE-ERET: user_sp={:#x} -> PA {:#x} pid={} OK",
                                usp, pa, t.process_id);
                        }
                        None => {
                            crate::log_error!(
                                "SCHED",
                                "PRE-ERET: user_sp={:#x} check={:#x} pid={} has no valid PTE!",
                                usp, check_va, t.process_id
                            );
                            // CANARY: dump PID 1's L3 entries at corruption time.
                            // Uses the dynamic WATCH_PA if set, else falls back to
                            // a direct page-table walk.
                            if t.process_id == 1 {
                                let watched = crate::arch::aarch64::phys::WATCH_PA.load(core::sync::atomic::Ordering::Relaxed);
                                if watched != 0 {
                                    unsafe {
                                        use crate::arch::mmu_facade::pa_to_kernel_va;
                                        let l3_kva = pa_to_kernel_va(watched) as *const u64;
                                        for ci in 0..8 {
                                            let val = core::ptr::read_volatile(l3_kva.add(ci));
                                            crate::log_error!("CANARY", "PID 1 L3[{}]={:#x} (watched pa={:#x})", ci, val, watched);
                                        }
                                    }
                                } else {
                                    // fallback: walk PID 1's page table directly
                                    if let Some((l0_pa, _)) = crate::task::process::find_process_l0_user_pa(1) {
                                        let stack_va = if let Some(p) = crate::task::process::find_process_mut(1) {
                                            p.root_vmar.base + 0x2000000
                                        } else { 0 };
                                        if stack_va != 0 {
                                            let pt = crate::arch::aarch64::mmu::translate_user_va(l0_pa, stack_va);
                                            crate::log_error!("CANARY", "PID 1 stack PT walk: {:#x} -> {:?}", stack_va, pt);
                                        }
                                    }
                                }
                            }
                            crate::log_info!(
                                "SCHED",
                                "  PT-VALIDATE pid={}: {}",
                                t.process_id,
                                if crate::task::process::validate_process_page_table(t.process_id)
                                    { "tree OK" } else { "** TREE CORRUPTED **" }
                            );
                        }
                    }
                }
            }

            self.unlock();

            unsafe {
                CurrentArch::switch_context(&mut *prev_context_ptr, &*next_context_ptr);
            }

            // Control returns here when this thread is scheduled back in
            // (i.e. the *next* thread from the call above is now the
            // previous one).  Restore TTBR0 to the original (now current)
            // process's L0 so the kernel can keep poking at the user
            // address space it was working on before the switch.  We do
            // exactly the same double-barrier dance as the outgoing path
            // (B1.3) so that the second process we switched *into* got
            // the same MMU-walker-visible flush on its way in.
            #[cfg(target_arch = "aarch64")]
            unsafe {
                crate::arch::CurrentArch::flush_tlb();

                // Restore TTBR0 to the original (now current) process's L0
                // so the CPU can translate user VAs through the correct table
                // when returning back to the user context!
                if let Some(ref t) = self.threads[prev_idx] {
                    if let Some((cur_l0_pa, cur_asid)) =
                        crate::task::process::find_process_l0_user_pa(t.process_id)
                    {
                        crate::arch::aarch64::mmu::set_ttbr0_el1(cur_l0_pa, cur_asid);
                    }
                }

                // Zircon-aligned Cache Hardening: Invalidate instruction pre-fetch pipeline
                // at the thread's user-mode entry PC (elr) to prevent QEMU TCG decoding translation
                // faults on the very first instruction fetch after eret!
                if let Some(ref mut t) = self.threads[prev_idx] {
                    let user_pc = t.context.elr;
                    if user_pc != 0 {
                        unsafe {
                            <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::instruction_barrier();
                        }
                    }
                }
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
                        crate::arch::CurrentArch::wait_for_event();
                    }
                }
            }

            self.current_idx = None;
        }
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
