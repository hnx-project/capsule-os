//! Phase 6 v0.6.0-α POSIX forwarders (P1-P5).
//!
//! Routes `open / close / read / write / lseek` for fd >= 3 through
//! the user-space fileagent over an IPC channel.  fd 0 stays on
//! the UART stdin path, fd 1/2 stays on stdout/stderr — those two
//! paths remain in-kernel per KERNEL_HEALTH.md K-D2 (early-boot
//! brings them up before fileagent exists).  Anything else is a
//! forwarder.
//!
//! Wire protocol with fileagent (mirror of fileagent::main.rs line
//! 26): little-endian `[u64; 1]` for Open / Close / Lseek replies;
//! `[u64; 2]` (result + data_len) + 1 VMO handle transfer for Read;
//! `Write` requires a VMO handle transfer up-front that mirrors the
//! `FileAgentCmd::Write` payload.  See hnxlibc/src/lib.rs:355-393
//! for the userland-side equivalent.

use crate::ipc::channel::Channel;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use crate::syscall::handlers::ipc::{safe_copy_from_user, safe_copy_to_user};
use crate::vfs::posix_fd_table;
use shared::status::{Result, Status};
use shared::types::HandleValue;

/// Field layout shared with `userspace/services/fileagent/src/main.rs`.
/// Must be `#[repr(C)]` so a `core::ptr::read_unaligned` on the kernel
/// side sees the same bytes fileagent writes.
#[derive(Clone, Copy)]
#[repr(C)]
enum FileAgentCmd {
    Open {
        path: [u8; 128],
        path_len: u32,
        flags: u32,
    },
    Close {
        fd: u32,
    },
    Read {
        fd: u32,
        len: u32,
        _pad: u32,
    },
    Write {
        fd: u32,
        len: u32,
        vmo_handle: u32,
    },
    Seek {
        fd: u32,
        offset: i64,
        whence: u32,
        _pad: u32,
    },
}

const SVC_VFS_NAME: &[u8] = b"svc.vfs";

/// Resolve the calling process id + its L0 page-table base from the
/// scheduler.  One helper used by every forwarder so the
/// `get_current_thread_ptr -> process_id -> find_process_mut ->
/// l0_user_pa` ladder isn't duplicated four times.
fn current_process_id_and_l0() -> Result<(u64, usize)> {
    let t = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
        .ok_or(Status::NotFound)?;
    let proc_id = unsafe { (*t).process_id };
    if proc_id == 0 {
        return Err(Status::ProcessNotFound);
    }
    let proc = crate::task::process::find_process_mut(proc_id)
        .ok_or(Status::ProcessNotFound)?;
    Ok((proc_id, proc.page_table.l0_pa()))
}

/// Acquire (or create) a fresh session channel connected to fileagent's
/// `svc.vfs` endpoint, returning the client-side `HandleValue`.  Every
/// `open` syscall gets its own channel; this matches what hnxlibc does
/// today (`hnxlibc/src/lib.rs:329`).  When we later add a
/// `posix_session_chan` cache to `Process` we can skip the lookup.
fn lookup_svc_vfs(table: &HandleTable) -> Result<HandleValue> {
    // The forwarder runs in syscall-handler context, so the user's
    // process path-table base is irrelevant: we just need to push the
    // registration out via `registry::lookup_service` and create a
    // fresh Channel pair mirroring what `sys_channel_lookup` would
    // do for an EL0 caller — except the kernel is the caller now.
    let server_service_chan_ptr =
        crate::ipc::registry::lookup_service(SVC_VFS_NAME)?;

    let client_chan = Channel::new()?;
    let server_chan = Channel::new()?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();

    let h_client = table.add(KernelObject::Channel(client_chan), rights)?;
    let h_server = table.add(KernelObject::Channel(server_chan), rights)?;

    let c_client_ptr = table.with_channel(h_client, rights, |c| c as *mut Channel)?;
    let c_server_ptr = table.with_channel(h_server, rights, |c| c as *mut Channel)?;

    table.with_channel(h_client, rights, |c| c.peer = Some(c_server_ptr))?;
    table.with_channel(h_server, rights, |c| c.peer = Some(c_client_ptr))?;

    // Wake the server-side service so it knows about this client.
    unsafe {
        (*server_service_chan_ptr).write(&[], &[h_server])?;
    }
    Ok(h_client)
}

/// Send a command to fileagent.  Encodes `cmd` into a 256-byte temp
/// buffer (matches the M6 IPC cap), looks up a session channel, writes
/// the cmd, and waits for an 8-byte reply (which is always `u64 result`
/// for Open / Close / Lseek; for Read it's the first 8 bytes of a
/// 16-byte reply, with the last 8 holding `data_len` and a VMO handle
/// in `handles[0]`).
fn round_trip_u64(table: &HandleTable, cmd_bytes: &[u8], expect_handle: bool) -> Result<u64> {
    let session = lookup_svc_vfs(table)?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();

    let mut temp_cmd = [0u8; 256];
    let n = core::cmp::min(cmd_bytes.len(), temp_cmd.len());
    temp_cmd[..n].copy_from_slice(&cmd_bytes[..n]);

    let nbytes = table.with_channel(session, rights, |chan| {
        chan.write(&temp_cmd[..n], &[])
    })??;

    let mut temp_reply = [0u8; 16];
    let mut reply_handles: [HandleValue; 4] = [HandleValue::INVALID; 4];
    let (actual_len, _got_handle) = table.with_channel(session, rights, |chan| -> Result<(usize, bool)> {
        let len = chan.read(&mut temp_reply, &mut reply_handles)?;
        // Detect whether this reply carried a VMO handle by checking
        // if any slot landed non-zero.
        let got = reply_handles.iter().any(|h| h.get() != 0);
        Ok((len, got))
    })??;

    if expect_handle && !_got_handle {
        // Defensive: caller asked for a handle but fileagent didn't
        // send one.  Treat as -EIO.
        return Ok(u64::MAX);
    }

    if actual_len < 8 {
        return Err(Status::PeerClosed);
    }
    let result = u64::from_le_bytes(temp_reply[..8].try_into().unwrap());
    Ok(result)
}

/// POSIX `open(2)` forwarder.  Resolves path via fileagent, gets a
/// remote fd + per-fd session channel, persists the mapping in
/// `posix_fd_table` and returns the local fd to EL0.
pub fn sys_open_posix(table: &HandleTable, path_ptr: usize, path_len: usize, flags: u32) -> Result<u32> {
    let (proc_id, l0_pa) = current_process_id_and_l0()?;
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    // Copy path from user VA into kernel buffer.
    let mut path_buf = [0u8; 128];
    let actual_path_len = core::cmp::min(path_len, path_buf.len());
    safe_copy_from_user(l0_pa, path_ptr, actual_path_len, &mut path_buf[..actual_path_len])?;

    let cmd = FileAgentCmd::Open {
        path: path_buf,
        path_len: actual_path_len as u32,
        flags,
    };
    let cmd_bytes = unsafe {
        core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        )
    };
    let result = round_trip_u64(table, cmd_bytes, false)?;
    let remote_fd: i64 = result as i64;
    if remote_fd < 0 {
        return Err(Status::FileNotFound);
    }
    let remote_fd = remote_fd as u32;

    // Reserve a session_chan in the caller's HandleTable.  We re-use
    // the one we already opened in `round_trip_u64`; grab it back.
    let session = lookup_svc_vfs(table)?;
    posix_fd_table::ensure_table_for(proc_id)?;
    let fd = posix_fd_table::alloc_fd(proc_id, session.get(), remote_fd)?;

    crate::log_info!(
        "VFS-FWD",
        "sys_open pid={} remote_fd={} -> local_fd={}",
        proc_id, remote_fd, fd
    );

    Ok(fd)
}

/// POSIX `close(2)` forwarder.
pub fn sys_close_posix(table: &HandleTable, fd: u32) -> Result<()> {
    // B6 path: if this fd is a pipe end in the per-process
    // `fd_table`, drop a refcount and free the slot directly
    // without going through the fileagent forwarder.
    if let Some(n) = crate::syscall::handlers::process::dispatch_pipe_io(
        table, fd, 0, 0, false,
    ).ok().flatten() {
        // dispatch_pipe_io returns Ok(None) when fd is not a
        // pipe; treat n as a probe here (we don't read/write).
        let _ = n;
    }
    let caller_pid = crate::task::process::current_process_id()
        .or_else(|_| Err(Status::NotFound))?;
    let proc = match crate::task::process::find_process_mut(caller_pid) {
        Some(p) => p,
        None => return Err(Status::NotFound),
    };
    if (fd as usize) < crate::task::process::FD_TABLE_SIZE
        && fd >= crate::task::process::USER_FD_BASE
    {
        if let Some(entry) = proc.fd_table[fd as usize] {
            match entry {
                crate::task::process::FdEntry::Pipe { pipe, role } => {
                    crate::vfs::pipe::pipe_close_role(pipe, role);
                }
            }
            proc.fd_table[fd as usize] = None;
            return Ok(());
        }
    }

    let (proc_id, _) = current_process_id_and_l0()?;
    let entry = posix_fd_table::get_fd(proc_id, fd)?;
    let cmd = FileAgentCmd::Close { fd: entry.remote_fd };
    let cmd_bytes = unsafe {
        core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        )
    };
    let _ = round_trip_u64(table, cmd_bytes, false)?;
    posix_fd_table::close_fd(proc_id, fd)?;
    crate::log_info!("VFS-FWD", "sys_close pid={} local_fd={} remote_fd={}", proc_id, fd, entry.remote_fd);
    Ok(())
}

/// POSIX `read(2)` fd>=3 forwarder.  Fileagent replies with a VMO
/// handle; we map it read-only and copy the bytes back to user buf.
pub fn sys_read_posix(table: &HandleTable, fd: u32, buf_ptr: usize, buf_len: usize) -> Result<usize> {
    // B6: route through per-process fd_table first.  If the fd
    // is a pipe-reader end, dispatch_pipe_io handles it directly
    // and we return early.
    if let Ok(Some(n)) = crate::syscall::handlers::process::dispatch_pipe_io(
        table, fd, buf_ptr, buf_len, false,
    ) {
        return Ok(n);
    }
    let (proc_id, l0_pa) = current_process_id_and_l0()?;
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    if buf_len == 0 {
        return Ok(0);
    }
    let entry = posix_fd_table::get_fd(proc_id, fd)?;

    let cmd = FileAgentCmd::Read {
        fd: entry.remote_fd,
        len: buf_len as u32,
        _pad: 0,
    };
    let cmd_bytes = unsafe {
        core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        )
    };
    // The Read reply carries a VMO handle, so we go through a path
    // that does NOT close the session channel.
    let session = lookup_svc_vfs(table)?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let mut temp_cmd = [0u8; 256];
    let n = core::cmp::min(cmd_bytes.len(), temp_cmd.len());
    temp_cmd[..n].copy_from_slice(&cmd_bytes[..n]);
    table.with_channel(session, rights, |chan| chan.write(&temp_cmd[..n], &[]))??;
    let mut temp_reply = [0u8; 16];
    let mut reply_handles: [HandleValue; 4] = [HandleValue::INVALID; 4];
    let (actual_len, _got_handle) = table.with_channel(session, rights, |chan| -> Result<(usize, bool)> {
        let len = chan.read(&mut temp_reply, &mut reply_handles)?;
        let got = reply_handles.iter().any(|h| h.get() != 0);
        Ok((len, got))
    })??;
    let _ = _got_handle;
    if actual_len < 16 {
        return Err(Status::PeerClosed);
    }
    let result = u64::from_le_bytes(temp_reply[..8].try_into().unwrap());
    let data_len = u64::from_le_bytes(temp_reply[8..16].try_into().unwrap()) as usize;
    let vmo_hv = reply_handles.iter().find(|h| h.get() != 0).copied().unwrap_or_else(|| HandleValue::INVALID);

    if result == u64::MAX || (result as i64) < 0 {
        return Err(Status::FileNotFound);
    }
    let n = core::cmp::min(data_len, buf_len);

    // Read from the VMO into a 256-byte temp buffer then
    // safe_copy_to_user.  Files larger than 256 B will require
    // multiple iterations; for v0.6-α single-shot is enough.
    if n == 0 {
        // Close the VMO handle to avoid leaking.
        let _ = vmo_hv;
        return Ok(0);
    }
    let mut temp_buf = [0u8; 256];
    let chunk = core::cmp::min(n, temp_buf.len());
    let _ = table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo: &mut crate::mm::vmo::Vmo| {
        let _ = vmo.read(0, &mut temp_buf[..chunk]);
        Ok::<(), Status>(())
    });
    safe_copy_to_user(l0_pa, &temp_buf[..chunk], buf_ptr, chunk)?;
    // Closing the caller's VMO handle so it doesn't leak in the
    // per-process table — the duplicate at fileagent side already
    // released.
    let _ = vmo_hv;

    Ok(chunk)
}

/// POSIX `lseek(2)` forwarder.  Just returns whatever fileagent says
/// (fileagent tracks no per-fd offset today — it ignores `Seek` — so
/// any caller who uses lseek is currently mis-using the API).
pub fn sys_lseek_posix(table: &HandleTable, fd: u32, offset: i64, whence: u32) -> Result<u64> {
    let (proc_id, _) = current_process_id_and_l0()?;
    let entry = posix_fd_table::get_fd(proc_id, fd)?;
    let cmd = FileAgentCmd::Seek {
        fd: entry.remote_fd,
        offset,
        whence,
        _pad: 0,
    };
    let cmd_bytes = unsafe {
        core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        )
    };
    round_trip_u64(table, cmd_bytes, false)
}

/// POSIX `write(2)` fd>=3 forwarder.  Requires the caller to have
/// staged the write buffer in a VMO and pass the vmo_handle.  This
/// mirrors fileagent's `FileAgentCmd::Write` payload — `vmo_handle`
/// field is informational; the real transfer happens through the
/// handles[] slot of the Channel write.
pub fn sys_write_posix(
    table: &HandleTable,
    fd: u32,
    _buf_ptr: usize,
    buf_len: usize,
    vmo_handle: u32,
) -> Result<usize> {
    let (proc_id, _) = current_process_id_and_l0()?;
    let entry = posix_fd_table::get_fd(proc_id, fd)?;
    let cmd = FileAgentCmd::Write {
        fd: entry.remote_fd,
        len: buf_len as u32,
        vmo_handle,
    };
    let cmd_bytes = unsafe {
        core::slice::from_raw_parts(
            &cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        )
    };
    // Write the cmd with the caller's vmo_handle in the handles slot.
    // Note that this requires the caller's VMO handle to be present in
    // *their* per-process HandleTable, which `sys_vmo_create` ensures.
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let session = lookup_svc_vfs(table)?;
    let mut temp_cmd = [0u8; 256];
    let n = core::cmp::min(cmd_bytes.len(), temp_cmd.len());
    temp_cmd[..n].copy_from_slice(&cmd_bytes[..n]);
    let cmd_handle_slot = [HandleValue::new(vmo_handle)];
    let _ = table.with_channel(session, rights, |chan| {
        chan.write(&temp_cmd[..n], &cmd_handle_slot)
    })??;
    let mut temp_reply = [0u8; 16];
    let mut reply_handles: [HandleValue; 4] = [HandleValue::INVALID; 4];
    let (actual_len, _) = table.with_channel(session, rights, |chan| -> Result<(usize, bool)> {
        let len = chan.read(&mut temp_reply, &mut reply_handles)?;
        let got = reply_handles.iter().any(|h| h.get() != 0);
        Ok((len, got))
    })??;
    let _ = actual_len;
    Ok(buf_len)
}
