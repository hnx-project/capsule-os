use shared::status::{Result, Status};
use core::sync::atomic::{AtomicU32, Ordering};

static PORT_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub const PORT_QUEUE_MAX: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct PortPacket {
    pub key: u64,
    pub trigger: u32,
    pub status: i32,
    pub bytes: [u8; 64],
}

impl Default for PortPacket {
    fn default() -> Self {
        PortPacket {
            key: 0,
            trigger: 0,
            status: 0,
            bytes: [0u8; 64],
        }
    }
}

#[derive(Debug)]
pub struct Port {
    pub id: u32,
    pub queue_len: u32,
    pub queue_va: usize, // Virtual address of the allocated physical page
    pub head: usize,
    pub tail: usize,
    pub waiter: Option<usize>,
}

impl Port {
    pub fn new(queue_len: u32) -> Result<Self> {
        if queue_len == 0 || queue_len > PORT_QUEUE_MAX as u32 {
            return Err(Status::InvalidArgs);
        }
        
        // Allocate a dedicated physical page for the queue
        let queue_pa = crate::mm::phys::alloc_page()?;
        let queue_va = crate::mm::mmu::pa_to_kernel_va(queue_pa.as_usize());
        
        // Initialize the slots on the allocated page to None
        unsafe {
            let p = queue_va as *mut Option<PortPacket>;
            for i in 0..PORT_QUEUE_MAX {
                core::ptr::write(p.add(i), None);
            }
        }

        Ok(Port {
            id: PORT_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            queue_len,
            queue_va,
            head: 0,
            tail: 0,
            waiter: None,
        })
    }

    pub fn wait(&mut self) -> Result<PortPacket> {
        let p = self.queue_va as *mut Option<PortPacket>;

        // 1. Check if there is already a packet in our ring buffer queue
        if self.head != self.tail {
            unsafe {
                if let Some(pkt) = (*p.add(self.head)).take() {
                    self.head = (self.head + 1) % PORT_QUEUE_MAX;
                    return Ok(pkt);
                }
            }
        }

        // 2. Queue is empty, register ourselves as waiter and block
        let cur_thread_ptr = unsafe {
            crate::task::scheduler::SCHEDULER.get_current_thread_mut().ok_or(Status::ThreadNotFound)? as *mut crate::task::thread::Thread
        };
        let cur_thread = unsafe { &mut *cur_thread_ptr };

        cur_thread.port_packet_slot = None;
        cur_thread.state = crate::task::thread::ThreadState::Blocked;
        self.waiter = Some(cur_thread.id);

        // Yield CPU
        unsafe {
            crate::task::scheduler::SCHEDULER.schedule();
        }

        // Once woken up, fetch packet from our TCB slot
        let woken_thread = unsafe { &mut *cur_thread_ptr };
        let pkt = woken_thread.port_packet_slot.take().ok_or(Status::Canceled)?;

        Ok(pkt)
    }

    pub fn queue(&mut self, packet: &PortPacket) -> Result<()> {
        // 1. Check if there is already a service thread waiting
        if let Some(waiter_tid) = self.waiter.take() {
            unsafe {
                if let Some(waiter) = crate::task::scheduler::SCHEDULER.get_thread_mut(waiter_tid) {
                    // Copy packet directly to waiter's TCB slot
                    waiter.port_packet_slot = Some(*packet);
                    waiter.state = crate::task::thread::ThreadState::Ready;
                    return Ok(());
                }
            }
        }

        // 2. No waiter. Put packet into the static ring buffer queue
        let next_tail = (self.tail + 1) % PORT_QUEUE_MAX;
        if next_tail == self.head {
            // Queue is full
            return Err(Status::TryAgain);
        }

        let p = self.queue_va as *mut Option<PortPacket>;
        unsafe {
            core::ptr::write(p.add(self.tail), Some(*packet));
        }
        self.tail = next_tail;

        Ok(())
    }
}
