use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::sync::futex::Futex;

pub struct Mutex {
    futex: Futex,
    state: AtomicU32,
}

impl Mutex {
    pub fn new() -> Self {
        Mutex {
            futex: Futex::new(),
            state: AtomicU32::new(0),
        }
    }

    pub fn lock(&self) -> Result<()> {
        let old = self.state.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed);
        if old.is_ok() {
            return Ok(());
        }

        self.futex.wait(self.state_addr(), 1, None)?;
        Ok(())
    }

    pub fn unlock(&self) {
        self.state.store(0, Ordering::Release);
        let _ = self.futex.wake(self.state_addr(), 1);
    }

    pub fn try_lock(&self) -> bool {
        self.state.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    fn state_addr(&self) -> usize {
        &self.state as *const AtomicU32 as usize
    }
}

pub struct Semaphore {
    futex: Futex,
    count: AtomicU32,
}

impl Semaphore {
    pub fn new(initial: u32) -> Self {
        Semaphore {
            futex: Futex::new(),
            count: AtomicU32::new(initial),
        }
    }

    pub fn acquire(&self) -> Result<()> {
        loop {
            let current = self.count.load(Ordering::SeqCst);
            if current > 0 {
                if self.count.compare_exchange(current, current - 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                    return Ok(());
                }
            } else {
                self.futex.wait(self.count_addr(), 0, None)?;
            }
        }
    }

    pub fn release(&self) {
        let current = self.count.fetch_add(1, Ordering::Release);
        if current == 0 {
            let _ = self.futex.wake(self.count_addr(), 1);
        }
    }

    pub fn try_acquire(&self) -> bool {
        let current = self.count.load(Ordering::SeqCst);
        if current > 0 {
            self.count.compare_exchange(current, current - 1, Ordering::Acquire, Ordering::Relaxed).is_ok()
        } else {
            false
        }
    }

    fn count_addr(&self) -> usize {
        &self.count as *const AtomicU32 as usize
    }
}

pub struct Event {
    futex: Futex,
    signaled: AtomicU32,
}

impl Event {
    pub fn new(auto_reset: bool) -> Self {
        Event {
            futex: Futex::new(),
            signaled: AtomicU32::new(if auto_reset { 0x8000_0000 } else { 0 }),
        }
    }

    pub fn signal(&self) {
        self.signaled.fetch_or(0x8000_0000, Ordering::Release);
        let _ = self.futex.wake(self.signaled_addr(), 1);
    }

    pub fn pulse(&self) -> bool {
        let old = self.signaled.fetch_and(!0x8000_0000, Ordering::Release);
        (old & 0x8000_0000) != 0
    }

    pub fn wait(&self) -> Result<()> {
        loop {
            let current = self.signaled.load(Ordering::SeqCst);
            if (current & 0x8000_0000) != 0 {
                let auto_reset = (current & 0x7FFF_FFFF) != 0;
                if auto_reset {
                    self.signaled.fetch_and(!0x8000_0000, Ordering::Release);
                }
                return Ok(());
            }
            self.futex.wait(self.signaled_addr(), 0, None)?;
        }
    }

    pub fn try_wait(&self) -> bool {
        let old = self.signaled.fetch_and(!0x8000_0000, Ordering::Acquire);
        (old & 0x8000_0000) != 0
    }

    pub fn ack(&self) {
        self.signaled.store(0, Ordering::Release);
    }

    fn signaled_addr(&self) -> usize {
        &self.signaled as *const AtomicU32 as usize
    }
}
