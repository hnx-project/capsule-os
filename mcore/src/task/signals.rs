//! B5 (`KERNEL_HEALTH.md` B5): POSIX-style signal model (1.0 subset).
//!
//! Scope of this implementation:
//!
//!   * `sigaction(sig, act, oldact)` - register disposition.  We
//!     accept `SIG_DFL (0)` or `SIG_IGN (1)` only; custom user-mode
//!     handlers would require a sigreturn trampoline and are not
//!     in 1.0 scope.
//!   * `raise(sig)` - self-targeted signal.
//!   * `kill(pid, sig)` - cross-process signal.
//!   * `pause()` - yield until a non-blocked, non-ignored signal is
//!     pending.
//!
//! Signal numbers are POSIX-style integers in `[1, 32]`.  We model
//! them as a `u32` bitfield on the process struct (bit `n - 1` for
//! signal number `n`), so 32 fits in a single word.
//!
//! ## Dispatch order
//!
//! The dispatcher walks the bitfield on every syscall_exit path
//! (`syscall_dispatch` after `handlers::*::...` returns), dequeuing
//! the lowest-numbered non-ignored, non-blocked signal.  For 1.0
//! "block mask" is *always* zero (sigprocmask is a stub), so any
//! SIG_DFL pending bit terminates the process and any SIG_IGN bit
//! is just dropped before we re-enter the bit loop.
//!
//! ## Why a separate file from `process.rs`
//!
//! The signal state-machine is non-trivial: bit-field iteration +
//! process state transitions + threading lock domain cross in.
//! Keeping it isolated leaves `process.rs` for the lifetime model
//! and `thread.rs` for the run queue.

use crate::task::process::{Process, ProcessState};
use shared::status::{Result, Status};

/// POSIX-style signal sentinel values that we accept as `sa_handler`
/// payloads.  Anything outside this set is rejected with
/// `InvalidArgs`.  The constants are deliberately `usize` because
/// POSIX `sighandler_t` is a function pointer.
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

/// NSIG for our 1.0 model.  We accept numbers in `[1, NSIG]`.
pub const NSIG: usize = 32;

/// Sentinel exit code we record when a process is killed by a
/// `SIG_DFL` disposition on a signal.  Linux folds the signo into
/// the upper 8 bits and the exit code into the lower 7 (`WTERMSIG`
/// etc.); we just store the signo itself so the parent wait4() can
/// distinguish "exited cleanly" from "killed by signal n".
const SIG_DFL_KILL_EXIT: i32 = -1;

/// Convert a signal number to its bit-field position (one-based
/// in -> zero-based bit position out).  Returns `Err` if the
/// signal number is out of range.
#[inline]
fn sig_to_bit(sig: usize) -> Result<u32> {
    if sig == 0 || sig > NSIG {
        return Err(Status::InvalidArgs);
    }
    Ok(1u32 << (sig as u32 - 1))
}

/// Set the disposition for `sig`.  Returns the previous disposition
/// in case the caller wants to chain (POSIX semantics).
pub fn sigaction_set(sig: usize, handler: usize) -> Result<usize> {
    if sig == 0 || sig > NSIG {
        return Err(Status::InvalidArgs);
    }
    if handler != SIG_DFL && handler != SIG_IGN {
        // We do not support user-mode trampolines in 1.0; see
        // the file-level documentation for the rationale
        // (sigreturn frame setup is its own KERNEL_HEALTH bucket
        // we have not yet opened).
        return Err(Status::InvalidArgs);
    }
    let caller_pid = crate::task::process::current_process_id()
        .or_else(|_| Err(Status::NotFound))?;
    let proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let idx = sig - 1;
    let prev = proc.sig_handlers[idx] as usize;
    proc.sig_handlers[idx] = handler as u8;
    Ok(prev)
}

/// Queue signal `sig` for delivery to process `target_pid`.  Returns
/// `Err(Status::NotFound)` if the process slot is empty.
pub fn signal_send(target_pid: u64, sig: usize) -> Result<()> {
    let bit = sig_to_bit(sig)?;
    let proc = crate::task::process::find_process_mut(target_pid)
        .ok_or(Status::NotFound)?;
    proc.pending_signals |= bit;
    Ok(())
}

/// "Raise" signal `sig` to the calling process.
pub fn raise(sig: usize) -> Result<()> {
    if sig == 0 || sig > NSIG {
        return Err(Status::InvalidArgs);
    }
    let caller = crate::task::process::current_process_id()
        .or_else(|_| Err(Status::NotFound))?;
    signal_send(caller, sig)
}

/// Apply pending signals for the calling process.  Called from
/// the trap path's "syscall return" branch; if a SIG_DFL disposition
/// matched we set the caller's exit_status + Zombie state so a
/// parent wait4() will reap it, and we ask the scheduler to drop
/// the caller in favour of something else.
///
/// Returns `Ok(true)` if a signal dispatched (caller should now
/// re-enter schedule rather than returning), or `Ok(false)` if no
/// signal was pending and the caller should continue as usual.
pub fn dispatch_pending(caller_pid: u64) -> Result<bool> {
    let proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let pending = proc.pending_signals;
    if pending == 0 {
        return Ok(false);
    }

    // Iterate bits 0..NSIG.  First non-ignored, non-default bit
    // found is the one we deliver.  SIG_DFL terminates the process
    // (so the bit is consumed below by marking the process
    // Zombie); SIG_IGN clears the bit and continues the scan.
    for n in 1..=NSIG {
        let bit = 1u32 << (n as u32 - 1);
        if pending & bit == 0 {
            continue;
        }
        let disposition = proc.sig_handlers[n - 1] as usize;
        // Consume the bit regardless.
        proc.pending_signals &= !bit;

        if disposition == SIG_IGN {
            continue;
        }
        // SIG_DFL: terminate the process.  We collapse the
        // signo into a fixed -1 exit code so the parent wait4
        // sees a single negative value with no rusage union
        // to interpret.  A full Linux `WTERMSIG/WIFEXITED`
        // split is added in a follow-up B5.1 commit.
        proc.exit_status = Some(SIG_DFL_KILL_EXIT);
        proc.state = ProcessState::Zombie;
        proc.pending_signals = 0u32;
        crate::log_info!(
            "SIGNAL",
            "pid={} received SIG{} (default disposition); zombie-out",
            caller_pid,
            n
        );
        return Ok(true);
    }

    // All bits were IGN or came from a stale slot; the kernel
    // can continue without stopping the caller.
    Ok(false)
}

/// Sets up a default disposition for `sig` when a process is
/// (re)spanned via the respawn path.  Centralised so the
/// init_respawn caller and the launch_user_program caller can
/// share.
pub fn reset_disposition(target_pid: u64) {
    if let Some(p) = crate::task::process::find_process_mut(target_pid) {
        p.sig_handlers = [0u8; 32];
        p.pending_signals = 0u32;
    }
}
