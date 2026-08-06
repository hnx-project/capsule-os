//! S8: Kernel log buffer syscalls.
//!
//! Routes the `SYSCALL_LOGBUF_*` and `SYSCALL_LOG_EMIT` numbers to
//! `kcore::logbuf`.  Mirrors the read pattern used by Linux's `kmsg_read`
//! / FreeBSD's `sysctl(kern.msgbuf)` / macOS's `os_log` archive readers.

use shared::status::Status;
use crate::kcore::{logbuf, logging};
use crate::syscall::handlers::ipc::{safe_copy_from_user, safe_copy_to_user};

/// `tail_seq()` from EL0.
pub fn sys_logbuf_tail(_arg0: usize, _arg1: usize) -> usize {
    logbuf::tail_seq() as usize
}

/// Drain records with `seq > since_seq` into the user buffer at `buf_ptr`
/// (max `buf_len` bytes).  Returns `(next_seq << 32) | bytes_written`.
pub fn sys_logbuf_read(since_seq: usize, buf_ptr: usize, buf_len: usize) -> usize {
    if buf_ptr == 0 || buf_len == 0 {
        return Status::InvalidArgs.to_raw();
    }
    let caller_pid = match crate::task::process::current_process_id() {
        Ok(p) => p,
        Err(_) => return Status::NotFound.to_raw(),
    };
    let l0_pa = match crate::task::process::find_process_mut(caller_pid) {
        Some(p) => p.page_table.l0_pa(),
        None => return Status::NotFound.to_raw(),
    };
    if l0_pa == 0 {
        return Status::InvalidArgs.to_raw();
    }

    let mut scratch = [0u8; 4096];
    let want = core::cmp::min(buf_len, scratch.len());
    let (next_seq, n) = logbuf::drain_to(since_seq as u64, &mut scratch[..want]);
    if n == 0 {
        return (next_seq as usize) << 32;
    }
    match safe_copy_to_user(l0_pa, &scratch[..n], buf_ptr, n) {
        Ok(()) => ((next_seq as usize) << 32) | n,
        Err(e) => e.to_raw(),
    }
}

/// Promote the kernel boot phase.  Phase ≥ 2 silences non-severe logs.
pub fn sys_logbuf_set_phase(phase: usize) -> usize {
    if phase > u8::MAX as usize {
        return Status::InvalidArgs.to_raw();
    }
    logging::set_boot_phase(phase as u8);
    0
}

/// Runtime knob for the console verbosity gate.
pub fn sys_logbuf_set_level(level: usize) -> usize {
    if level > 3 {
        return Status::InvalidArgs.to_raw();
    }
    logging::set_log_level(level as u8);
    0
}

/// EL0 log forwarder.  Copies the `(target, msg)` pair from the
/// caller's user VAs into kernel-side buffers and pushes them through
/// the same gate the kernel's own `log_*!` macros use.  Returns 0 on
/// success or a Status on failure.
pub fn sys_log_emit(
    level: usize,
    target_ptr: usize,
    target_len: usize,
    msg_ptr: usize,
    msg_len: usize,
) -> usize {
    if level > 3 || target_len > 16 || msg_len > 200 {
        return Status::InvalidArgs.to_raw();
    }
    let caller_pid = match crate::task::process::current_process_id() {
        Ok(p) => p,
        Err(_) => return Status::NotFound.to_raw(),
    };
    let l0_pa = match crate::task::process::find_process_mut(caller_pid) {
        Some(p) => p.page_table.l0_pa(),
        None => return Status::NotFound.to_raw(),
    };
    if l0_pa == 0 {
        return Status::InvalidArgs.to_raw();
    }

    let mut target_buf = [0u8; 16];
    let mut msg_buf = [0u8; 200];
    if safe_copy_from_user(l0_pa, target_ptr, target_len, &mut target_buf).is_err() {
        return Status::InvalidArgs.to_raw();
    }
    if safe_copy_from_user(l0_pa, msg_ptr, msg_len, &mut msg_buf).is_err() {
        return Status::InvalidArgs.to_raw();
    }

    let target = match core::str::from_utf8(&target_buf[..target_len]) {
        Ok(s) => s,
        Err(_) => return Status::InvalidArgs.to_raw(),
    };
    let msg = match core::str::from_utf8(&msg_buf[..msg_len]) {
        Ok(s) => s,
        Err(_) => return Status::InvalidArgs.to_raw(),
    };

    // Bypass `LogCapture` and apply the gate manually so we can match
    // the kernel's boot-phase / log-level semantics for EL0 traffic.
    use crate::kcore::logbuf::log_to_ring;
    use crate::kcore::logging::{boot_phase, get_log_level, ConsoleWriter, LEVEL_WARN};
    use core::fmt::Write;
    log_to_ring(level as u8, target, msg);

    let boot_phase = boot_phase();
    let allowed_by_phase = match boot_phase {
        0 | 1 => true,
        _ => level <= LEVEL_WARN as usize,
    };
    if allowed_by_phase && get_log_level() >= level as u8 {
        let prefix = match level {
            0 => "\x1b[1;31mERROR\x1b[0m | \x1b[36m",
            1 => "\x1b[1;33mWARN \x1b[0m | \x1b[36m",
            2 => "\x1b[1;32mINFO \x1b[0m | \x1b[36m",
            _ => "\x1b[90mDEBUG\x1b[0m | \x1b[36m",
        };
        let mut w = ConsoleWriter;
        let _ = w.write_str(prefix);
        let _ = w.write_str(target);
        let _ = w.write_str("\x1b[0m | ");
        let _ = w.write_str(msg);
        let _ = w.write_str("\n");
    }

    0
}