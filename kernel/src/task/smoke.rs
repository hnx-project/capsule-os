use crate::task::{Process, Thread};
use crate::syscall::set_handle_table;
use shared::status::{Result, Status};

static mut TEST_CHANNEL: Option<crate::ipc::Channel> = None;
static mut TEST_PORT: Option<crate::ipc::port::Port> = None;

extern "C" fn init_entry() {
    // Create a Vmo via syscall
    let vmo_handle_raw = crate::syscall::syscall_dispatch(
        crate::syscall::numbers::SYSCALL_VMO_CREATE, 16 * 1024, 0, 0, 0, 0, 0
    );
    crate::log_info!("TEST_A", "init_entry: Created VMO via syscall -> handle = {}", vmo_handle_raw);
    
    // Write data into Vmo via syscall
    let pattern = b"VMO_TRANSFER_OK";
    let _written = crate::syscall::syscall_dispatch(
        crate::syscall::numbers::SYSCALL_VMO_WRITE, vmo_handle_raw, 0, pattern.as_ptr() as usize, pattern.len(), 0, 0
    );
    crate::log_info!("TEST_A", "init_entry: Wrote '{}' to VMO", core::str::from_utf8(pattern).unwrap());

    // Write to channel and transfer the VMO handle!
    crate::log_info!("TEST_A", "init_entry: Writing to channel and transferring VMO handle...");
    let msg = b"HELLO_VMO_TRANS";
    let handles_to_send = [shared::types::HandleValue::new(vmo_handle_raw as u32)];
    let bytes_written = unsafe { TEST_CHANNEL.as_mut().unwrap().write(msg, &handles_to_send) }.unwrap_or(0);
    crate::log_info!("TEST_A", "init_entry: Woken up! Bytes written = {}", bytes_written);

    // Verify VMO handle is revoked
    let mut dummy_buf = [0u8; 16];
    let test_res = crate::syscall::syscall_dispatch(
        crate::syscall::numbers::SYSCALL_VMO_READ, vmo_handle_raw, 0, dummy_buf.as_mut_ptr() as usize, 16, 0, 0
    );
    if test_res == shared::status::Status::BadHandle.to_raw() {
        crate::log_info!("TEST_A", "init_entry: Revocation proof PASS! VMO handle is no longer accessible by init!");
    } else {
        crate::log_error!("TEST_A", "init_entry: Revocation proof FAIL! VMO handle is still accessible!");
    }

    // --- Phase 4.3 Async Completion Port test ---
    crate::log_info!("TEST_A", "init_entry: Waiting for events on Port (will block)...");
    let packet = unsafe { TEST_PORT.as_mut().unwrap().wait() }.unwrap();
    crate::log_info!("TEST_A", "init_entry: Port event received! key={:#x}, trigger={}", packet.key, packet.trigger);

    loop {}
}

extern "C" fn worker_entry() {
    crate::log_info!("TEST_B", "worker_entry: Reading from channel (will wake up init and receive VMO)...");
    let mut msg_buf = [0u8; 32];
    let mut recv_handles = [shared::types::HandleValue::new(0); 2];
    let bytes_read = unsafe { TEST_CHANNEL.as_mut().unwrap().read(&mut msg_buf, &mut recv_handles) }.unwrap_or(0);
    crate::log_info!("TEST_B", "worker_entry: Read successful! Msg = '{}'", core::str::from_utf8(&msg_buf[..bytes_read]).unwrap_or("?"));

    // Access the transferred VMO!
    let received_vmo_handle = recv_handles[0].get();
    crate::log_info!("TEST_B", "worker_entry: Received transferred VMO! New handle assigned to worker = {}", received_vmo_handle);

    let mut vmo_buf = [0u8; 16];
    let read_res = crate::syscall::syscall_dispatch(
        crate::syscall::numbers::SYSCALL_VMO_READ, received_vmo_handle as usize, 0, vmo_buf.as_mut_ptr() as usize, 16, 0, 0
    );
    let vmo_str = core::str::from_utf8(&vmo_buf[..read_res]).unwrap_or("?");
    crate::log_info!("TEST_B", "worker_entry: Read from transferred VMO successful! Content = '{}'", vmo_str);

    // --- Phase 4.3 Async Completion Port test ---
    crate::log_info!("TEST_B", "worker_entry: Queueing an asynchronous PortPacket event...");
    let mut packet = crate::ipc::port::PortPacket::default();
    packet.key = 0x1234_5678;
    packet.trigger = 42;
    unsafe { TEST_PORT.as_mut().unwrap().queue(&packet) }.unwrap();

    loop {}
}

/// Phase 3.3: exercise the handle table through syscall dispatch.
pub fn handle_smoke_test() {
    use crate::syscall::numbers::*;

    // 1. Create a VMO via the handle table.
    let hv = crate::syscall::syscall_dispatch(SYSCALL_VMO_CREATE, 16 * 1024, 0, 0, 0, 0, 0);
    if hv == Status::Ok.to_raw() {
        crate::log_error!("HANDLE", "vmo_create: FAILED (returned Ok)");
        return;
    }
    crate::log_info!("HANDLE", "vmo_create -> handle={}", hv);

    // 2. Write data via the handle table.
    let pattern = b"HANDLE_OK";
    let written = crate::syscall::syscall_dispatch(
        SYSCALL_VMO_WRITE, hv, 0, pattern.as_ptr() as usize, pattern.len(), 0, 0,
    );
    if (written as isize) < 0 {
        crate::log_error!("HANDLE", "sys_vmo_write failed: {:?}", Status::from_raw(written as i32));
        return;
    }
    crate::log_info!("HANDLE", "=> sys_vmo_write: {} bytes", written);

    // 3. Read it back.
    let mut buf = [0u8; 16];
    let read_n = crate::syscall::syscall_dispatch(
        SYSCALL_VMO_READ, hv, 0, buf.as_mut_ptr() as usize, buf.len(), 0, 0,
    );
    if (read_n as isize) < 0 {
        crate::log_error!("HANDLE", "sys_vmo_read failed: {:?}", Status::from_raw(read_n as i32));
        return;
    }
    let read_slice = &buf[..core::cmp::min(read_n, buf.len())];
    crate::log_info!("HANDLE", "=> sys_vmo_read: {} bytes, got: {}", read_n, core::str::from_utf8(read_slice).unwrap_or("?"));

    if &buf[..pattern.len()] == pattern {
        crate::log_info!("HANDLE", "smoke test PASS");
    } else {
        crate::log_error!("HANDLE", "smoke test FAIL (data mismatch)");
    }
}

pub fn launch_smoke_tests() -> Result<()> {
    unsafe {
        TEST_CHANNEL = Some(crate::ipc::Channel::new().unwrap());
        TEST_PORT = Some(crate::ipc::port::Port::new(8).unwrap());
    }

    let proc_init = Process::new("head")?;
    let proc_worker = Process::new("worker")?;

    let mut init_thread = Thread::new_kernel("head", init_entry)?;
    let mut worker_thread = Thread::new_kernel("worker", worker_entry)?;

    // Bind each thread to its own Process's HandleTable for true isolation!
    init_thread.process_id = proc_init.id;
    init_thread.handle_table = &proc_init.handle_table;

    worker_thread.process_id = proc_worker.id;
    worker_thread.handle_table = &proc_worker.handle_table;

    crate::log_info!("TASK", "init & worker threads created (with isolated process handle tables)");

    // Set state to Ready so they can be scheduled
    init_thread.state = crate::task::thread::ThreadState::Ready;
    worker_thread.state = crate::task::thread::ThreadState::Ready;

    // Try launching the user-space loader
    match crate::loader::launch_loader() {
        Ok(_) => {
            crate::log_info!("BOOT", "Loader process ready to schedule");
        }
        Err(e) => {
            crate::log_warn!("BOOT", "Loader skipped or failed ({:?}), falling back to kernel smoke threads", e);
            unsafe {
                crate::task::scheduler::SCHEDULER.add(init_thread);
                crate::task::scheduler::SCHEDULER.add(worker_thread);
            }
        }
    }

    // Wire syscall dispatch to the init process by default
    set_handle_table(&proc_init.handle_table);
    crate::log_info!("HANDLE", "handle table registered");
    
    // Exercise the handle table through syscall dispatch
    handle_smoke_test();

    Ok(())
}
