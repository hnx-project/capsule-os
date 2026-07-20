use core::sync::atomic::{AtomicU32, Ordering};
use shared::status::{Result, Status};
use crate::task::thread::ThreadState;
use crate::task::scheduler::SCHEDULER;

pub const MAX_FUTEX_WAITERS: usize = 8;

#[derive(Clone, Copy)]
pub struct FutexWaiter {
    pub thread_id: usize,
    pub next: Option<usize>,
}

pub struct FutexEntry {
    pub addr: usize,
    pub waiters: [Option<FutexWaiter>; MAX_FUTEX_WAITERS],
    pub waiter_count: usize,
    pub head: Option<usize>,
    pub tail: Option<usize>,
}

impl FutexEntry {
    const fn new(addr: usize) -> Self {
        FutexEntry {
            addr,
            waiters: [const { None }; MAX_FUTEX_WAITERS],
            waiter_count: 0,
            head: None,
            tail: None,
        }
    }
}

pub struct FutexTable {
    entries: [Option<FutexEntry>; 64],
    initialized: bool,
}

impl FutexTable {
    pub const fn new() -> Self {
        FutexTable {
            entries: [const { None }; 64],
            initialized: false,
        }
    }

    fn hash_addr(addr: usize) -> usize {
        (addr >> 3) % 64
    }

    fn get_or_create_entry(&mut self, addr: usize) -> &mut FutexEntry {
        let idx = Self::hash_addr(addr);
        if self.entries[idx].is_none() {
            self.entries[idx] = Some(FutexEntry::new(addr));
        }
        self.entries[idx].as_mut().unwrap()
    }

    fn ensure_initialized(&mut self) {
        if !self.initialized {
            *self = Self::new();
            self.initialized = true;
        }
    }

    fn find_entry_mut(&mut self, addr: usize) -> Option<&mut FutexEntry> {
        let idx = Self::hash_addr(addr);
        self.entries[idx].as_mut()
    }

    fn add_waiter(&mut self, entry: &mut FutexEntry, thread_id: usize) -> bool {
        if entry.waiter_count >= MAX_FUTEX_WAITERS {
            return false;
        }

        let waiter = FutexWaiter {
            thread_id,
            next: None,
        };

        for i in 0..MAX_FUTEX_WAITERS {
            if entry.waiters[i].is_none() {
                entry.waiters[i] = Some(waiter);
                entry.waiter_count += 1;

                if entry.head.is_none() {
                    entry.head = Some(i);
                    entry.tail = Some(i);
                } else if let Some(tail_idx) = entry.tail {
                    entry.waiters[tail_idx].as_mut().map(|t| t.next = Some(i));
                    entry.tail = Some(i);
                }
                return true;
            }
        }
        false
    }

    fn remove_waiter(&mut self, entry: &mut FutexEntry, thread_id: usize) {
        let mut prev_idx: Option<usize> = None;
        let mut curr_idx = entry.head;

        while let Some(idx) = curr_idx {
            if let Some(waiter) = entry.waiters[idx].as_ref() {
                if waiter.thread_id == thread_id {
                    let next_idx = waiter.next;

                    if let Some(prev) = prev_idx {
                        entry.waiters[prev].as_mut().map(|t| t.next = next_idx);
                    } else {
                        entry.head = next_idx;
                    }

                    if entry.tail == Some(idx) {
                        entry.tail = prev_idx;
                    }

                    entry.waiters[idx] = None;
                    entry.waiter_count -= 1;
                    return;
                }
            }
            prev_idx = Some(idx);
            curr_idx = entry.waiters[idx].as_ref().and_then(|w| w.next);
        }
    }

    fn wake_from_entry(entry: &mut FutexEntry, count: usize) -> usize {
        let mut woken = 0;
        let mut curr_idx = entry.head;

        while let Some(idx) = curr_idx {
            if woken >= count {
                break;
            }

            let waiter = entry.waiters[idx].take();
            if let Some(w) = waiter {
                curr_idx = w.next;
                unsafe { SCHEDULER.wake_thread(w.thread_id); }
                woken += 1;
            } else {
                break;
            }
        }

        if woken > 0 {
            entry.head = None;
            entry.tail = None;
            entry.waiter_count = 0;
        }

        woken
    }
}

static mut FUTEX_TABLE: FutexTable = FutexTable::new();

pub struct Futex;

impl Futex {
    pub fn new() -> Self {
        Futex
    }

    pub fn wait(&self, addr: usize, expected: u32, _deadline: Option<u64>) -> Result<()> {
        let current_val = unsafe { *(addr as *const u32) };

        if current_val != expected {
            return Ok(());
        }

        let thread_ptr = unsafe { SCHEDULER.get_current_thread_ptr() }
            .ok_or(Status::ThreadNotFound)?;

        let thread_id = unsafe { (*thread_ptr).id };

        unsafe {
            let entry = FUTEX_TABLE.get_or_create_entry(addr);

            if !FUTEX_TABLE.add_waiter(entry, thread_id) {
                return Err(Status::NoMemory);
            }

            (*thread_ptr).state = ThreadState::Blocked;
            SCHEDULER.schedule();

            FUTEX_TABLE.remove_waiter(entry, thread_id);
        }

        Ok(())
    }

    pub fn wake(&self, addr: usize, count: usize) -> Result<usize> {
        unsafe {
            let entry = match FUTEX_TABLE.find_entry_mut(addr) {
                Some(e) => e,
                None => return Ok(0),
            };

            if entry.head.is_none() {
                return Ok(0);
            }

            let woken = FutexTable::wake_from_entry(entry, count);
            Ok(woken)
        }
    }

    pub fn wake_all(&self, addr: usize) -> Result<usize> {
        self.wake(addr, usize::MAX)
    }

    pub fn requeue(&self, addr: usize, count: usize, _new_addr: usize) -> Result<usize> {
        unsafe {
            let entry = match FUTEX_TABLE.find_entry_mut(addr) {
                Some(e) => e,
                None => return Ok(0),
            };

            if entry.head.is_none() {
                return Ok(0);
            }

            let mut woken = 0;
            let mut curr_idx = entry.head;

            while let Some(idx) = curr_idx {
                if woken >= count {
                    break;
                }

                let waiter = entry.waiters[idx].take();
                if let Some(w) = waiter {
                    SCHEDULER.wake_thread(w.thread_id);
                    woken += 1;
                    curr_idx = w.next;
                } else {
                    break;
                }
            }

            if woken > 0 {
                entry.head = None;
                entry.tail = None;
                entry.waiter_count = 0;
            }

            Ok(woken)
        }
    }
}
