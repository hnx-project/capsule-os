use shared::status::{Result, Status};
use core::sync::atomic::{AtomicU32, Ordering};
use shared::types::HandleValue;

static CHANNEL_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub const MAX_WAITERS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState { Open, HalfClosed, Closed }

#[derive(Debug)]
pub struct Channel {
    pub id: u32,
    pub state: ChannelState,
    pub read_refs: AtomicU32,
    pub write_refs: AtomicU32,
    
    // Waiting queues storing Thread IDs
    pub send_waiters: [Option<usize>; MAX_WAITERS],
    pub recv_waiters: [Option<usize>; MAX_WAITERS],
}

impl Channel {
    pub fn new() -> Result<Self> {
        Ok(Channel {
            id: CHANNEL_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            state: ChannelState::Open,
            read_refs: AtomicU32::new(1),
            write_refs: AtomicU32::new(1),
            send_waiters: [None; MAX_WAITERS],
            recv_waiters: [None; MAX_WAITERS],
        })
    }

    fn push_sender(&mut self, tid: usize) -> bool {
        for slot in self.send_waiters.iter_mut() {
            if slot.is_none() {
                *slot = Some(tid);
                return true;
            }
        }
        false
    }

    fn pop_sender(&mut self) -> Option<usize> {
        for i in 0..MAX_WAITERS {
            if let Some(tid) = self.send_waiters[i] {
                self.send_waiters[i] = None;
                // Shift remaining waiters left to preserve FIFO
                for j in i..MAX_WAITERS-1 {
                    self.send_waiters[j] = self.send_waiters[j+1];
                }
                self.send_waiters[MAX_WAITERS-1] = None;
                return Some(tid);
            }
        }
        None
    }

    fn push_receiver(&mut self, tid: usize) -> bool {
        for slot in self.recv_waiters.iter_mut() {
            if slot.is_none() {
                *slot = Some(tid);
                return true;
            }
        }
        false
    }

    fn pop_receiver(&mut self) -> Option<usize> {
        for i in 0..MAX_WAITERS {
            if let Some(tid) = self.recv_waiters[i] {
                self.recv_waiters[i] = None;
                // Shift remaining waiters left to preserve FIFO
                for j in i..MAX_WAITERS-1 {
                    self.recv_waiters[j] = self.recv_waiters[j+1];
                }
                self.recv_waiters[MAX_WAITERS-1] = None;
                return Some(tid);
            }
        }
        None
    }

    pub fn read(&mut self, data: &mut [u8], handles: &mut [HandleValue]) -> Result<usize> {
        if self.state == ChannelState::Closed {
            return Err(Status::PeerClosed);
        }

        // Get our own thread mut
        let cur_thread_ptr = unsafe {
            crate::task::scheduler::SCHEDULER.get_current_thread_mut().ok_or(Status::ThreadNotFound)? as *mut crate::task::thread::Thread
        };
        let cur_thread = unsafe { &mut *cur_thread_ptr };

        // 1. Check if there is a waiting sender
        if let Some(sender_tid) = self.pop_sender() {
            unsafe {
                if let Some(sender) = crate::task::scheduler::SCHEDULER.get_thread_mut(sender_tid) {
                    // Copy directly from sender's registered buffer
                    let src_slice = core::slice::from_raw_parts(sender.ipc_buf_ptr as *const u8, sender.ipc_buf_len);
                    let copy_len = core::cmp::min(src_slice.len(), data.len());
                    data[..copy_len].copy_from_slice(&src_slice[..copy_len]);

                    // Handle Transfer: take stashed handles from sender and inject to our handle table
                    if !cur_thread.handle_table.is_null() {
                        let mut h_idx = 0;
                        for slot in sender.ipc_transfer_slots.iter_mut() {
                            if let Some((obj, rights)) = slot.take() {
                                if let Ok(new_hv) = (&*cur_thread.handle_table).add(obj, rights) {
                                    cur_thread.ipc_transfer_handles[h_idx] = Some(new_hv.get());
                                    h_idx += 1;
                                }
                            }
                        }
                    }

                    // Update sender's result and wake them up
                    sender.ipc_actual_len = copy_len;
                    sender.state = crate::task::thread::ThreadState::Ready;

                    // Extract the newly injected handles to output slice
                    let mut h_copied = 0;
                    for i in 0..handles.len() {
                        if i < cur_thread.ipc_transfer_handles.len() {
                            if let Some(h_raw) = cur_thread.ipc_transfer_handles[i].take() {
                                handles[i] = HandleValue::new(h_raw);
                                h_copied += 1;
                            }
                        }
                    }

                    return Ok(copy_len);
                }
            }
        }

        // 2. No sender is waiting. We register ourselves and block
        cur_thread.ipc_buf_ptr = data.as_mut_ptr() as usize;
        cur_thread.ipc_buf_len = data.len();
        cur_thread.ipc_actual_len = 0;
        cur_thread.state = crate::task::thread::ThreadState::Blocked;
        for h in cur_thread.ipc_transfer_handles.iter_mut() {
            *h = None;
        }

        let tid = cur_thread.id;
        if !self.push_receiver(tid) {
            cur_thread.state = crate::task::thread::ThreadState::Running;
            return Err(Status::NoMemory);
        }

        // Yield CPU
        unsafe {
            crate::task::scheduler::SCHEDULER.schedule();
        }

        // Once woken up, fetch actual bytes transferred and extract handles
        let woken_thread = unsafe { &mut *cur_thread_ptr };
        let actual_len = woken_thread.ipc_actual_len;

        let mut h_copied = 0;
        for i in 0..handles.len() {
            if i < woken_thread.ipc_transfer_handles.len() {
                if let Some(h_raw) = woken_thread.ipc_transfer_handles[i].take() {
                    handles[i] = HandleValue::new(h_raw);
                    h_copied += 1;
                }
            }
        }

        Ok(actual_len)
    }

    pub fn write(&mut self, data: &[u8], handles: &[HandleValue]) -> Result<usize> {
        if self.state == ChannelState::Closed {
            return Err(Status::PeerClosed);
        }

        // Get our own thread mut
        let cur_thread_ptr = unsafe {
            crate::task::scheduler::SCHEDULER.get_current_thread_mut().ok_or(Status::ThreadNotFound)? as *mut crate::task::thread::Thread
        };
        let cur_thread = unsafe { &mut *cur_thread_ptr };

        // 1. Check if there is a waiting receiver
        if let Some(receiver_tid) = self.pop_receiver() {
            unsafe {
                if let Some(receiver) = crate::task::scheduler::SCHEDULER.get_thread_mut(receiver_tid) {
                    // Copy directly to receiver's registered buffer
                    let dest_slice = core::slice::from_raw_parts_mut(receiver.ipc_buf_ptr as *mut u8, receiver.ipc_buf_len);
                    let copy_len = core::cmp::min(data.len(), dest_slice.len());
                    dest_slice[..copy_len].copy_from_slice(&data[..copy_len]);

                    // Handle Transfer: take handles from current thread's table and inject directly into receiver's table
                    if !cur_thread.handle_table.is_null() && !receiver.handle_table.is_null() {
                        let mut h_idx = 0;
                        for &h_val in handles.iter().take(2) {
                            if let Ok((obj, rights)) = (&*cur_thread.handle_table).remove_with_rights(h_val) {
                                if let Ok(new_hv) = (&*receiver.handle_table).add(obj, rights) {
                                    receiver.ipc_transfer_handles[h_idx] = Some(new_hv.get());
                                    h_idx += 1;
                                }
                            }
                        }
                    }

                    // Update receiver's result and wake them up
                    receiver.ipc_actual_len = copy_len;
                    receiver.state = crate::task::thread::ThreadState::Ready;

                    return Ok(copy_len);
                }
            }
        }

        // 2. No receiver is waiting. We stash our handles and block
        cur_thread.ipc_buf_ptr = data.as_ptr() as usize;
        cur_thread.ipc_buf_len = data.len();
        cur_thread.ipc_actual_len = 0;
        cur_thread.state = crate::task::thread::ThreadState::Blocked;
        
        // Remove and stash handles inside our own TCB slots
        for slot in cur_thread.ipc_transfer_slots.iter_mut() {
            *slot = None;
        }
        if !cur_thread.handle_table.is_null() {
            let mut h_idx = 0;
            for &h_val in handles.iter().take(2) {
                if let Ok((obj, rights)) = unsafe { &*cur_thread.handle_table }.remove_with_rights(h_val) {
                    cur_thread.ipc_transfer_slots[h_idx] = Some((obj, rights));
                    h_idx += 1;
                }
            }
        }

        let tid = cur_thread.id;
        if !self.push_sender(tid) {
            // Restore stashed handles on failure
            if !cur_thread.handle_table.is_null() {
                for slot in cur_thread.ipc_transfer_slots.iter_mut() {
                    if let Some((obj, rights)) = slot.take() {
                        let _ = unsafe { &*cur_thread.handle_table }.add(obj, rights);
                    }
                }
            }
            cur_thread.state = crate::task::thread::ThreadState::Running;
            return Err(Status::NoMemory);
        }

        // Yield CPU
        unsafe {
            crate::task::scheduler::SCHEDULER.schedule();
        }

        // Once woken up, fetch actual bytes transferred
        let woken_thread = unsafe { &mut *cur_thread_ptr };
        let actual_len = woken_thread.ipc_actual_len;

        Ok(actual_len)
    }

    pub fn close(&mut self) {
        self.state = ChannelState::Closed;
    }
}
