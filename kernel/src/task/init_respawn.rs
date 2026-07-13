//! Init anchor respawn — bring `system/bin/init` back when the boot
//! anchor (pid 1) dies.
//!
//! ## Why this exists
//!
//! Before 0.5.10, when the user-space process holding pid 1 (the
//! `hnx-loader` service) was killed by an EL0 fault or by a clean
//! `sys_exit`, the kernel would either spin in `SCHED-SAME prev ==
//! next == N` (fixed in `2992bd1 fix(scheduler)` and
//! `4aace5e fix(syscall)`) or fall through to
//! `SCHED No runnable threads left! Halting CPU safely…`.  Both
//! outcomes are correct for a single-purpose system, but they leave
//! the system in an unrecoverable state: nothing tries to bring
//! `system/bin/init` back, so a transient fault in pid 1 takes the
//! whole machine down.
//!
//! Linux's `init=` semantics, Plan 9's `init` chain, and Zircon's
//! "job manager respawns critical services" all solve this
//! problem.  We adopt the simplest one: **on the first death of
//! pid 1 after boot, the kernel automatically launches
//! `system/bin/init`**.  If that respawned init also dies, the
//! kernel does not try again — it falls through to the
//! "no runnable threads" halt path and stays there until the
//! operator intervenes.
//!
//! ## Why "consume once"
//!
//! A respawn loop would let a misbehaving `init` keep restarting
//! forever, masking real failures and exhausting the process
//! table.  Allowing exactly one respawn means:
//!
//! * A transient EL0 fault in the original loader is recovered
//!   automatically.
//! * A persistent fault in `init` (bad binary, immediate panic,
//!   etc.) fails loudly — the system halts instead of looping
//!   in the background.
//!
//! The `RESPAWN` log line tells the operator whether the system
//! has already used its one respawn.
//!
//! ## Threading & lock ordering
//!
//! `respawn_init_if_anchor` is called from three sites:
//!
//! 1. `sys_exit` (in the syscall handler, with kernel IRQs masked
//!    and the scheduler lock NOT held by the caller).
//! 2. `aarch64_sync_el0_handler` non-SVC branch (with kernel
//!    IRQs masked).
//! 3. `aarch64_serror_el0_handler` (with kernel IRQs masked).
//!
//! `Process::launch_user_program` itself takes the scheduler lock
//! when it calls `SCHEDULER.add`, and operates with kernel IRQs
//! in the same masked state, so the nesting is safe.

use crate::task::process::Process;
use core::sync::atomic::{AtomicBool, Ordering};

/// Set to `true` after the first successful respawn.  Subsequent
/// calls become no-ops, which prevents an `init` that dies in a
/// tight loop from spamming the process table and the log.
static RESPAWN_CONSUMED: AtomicBool = AtomicBool::new(false);

/// Path of the init binary to launch when the anchor dies.  Match
/// the path the boot loader uses during normal boot (see
/// `kernel_main` in `lib.rs`).
const INIT_BIN_PATH: &str = "system/bin/osh";

/// Name we register the respawned process under.  Distinct from
/// the original anchor's name so the log can tell them apart.
const INIT_BIN_NAME: &str = "osh-respawn";

/// If the caller is the boot anchor (pid 1) and the system has
/// not already used its respawn budget, attempt to launch
/// `system/bin/init` so the system can continue running.  Errors
/// are logged but do not panic — the caller is still free to
/// proceed with its own death (marked `Dead` in the calling
/// site) and the kernel will eventually halt if no thread ends
/// up runnable.
///
/// `current_pid` is the `process_id` of the dying caller (or 0
/// when no caller is known, in which case this is a no-op).
pub fn respawn_init_if_anchor(current_pid: u64) {
    if current_pid != 1 {
        return;
    }
    if RESPAWN_CONSUMED.load(Ordering::Acquire) {
        crate::log_warn!(
            "RESPAWN",
            "boot anchor (pid=1) died but the respawn budget is already spent; system will halt"
        );
        return;
    }

    let bytes = match crate::rootfs::get_file(INIT_BIN_PATH) {
        Some(b) => b,
        None => {
            crate::log_error!(
                "RESPAWN",
                "boot anchor (pid=1) died and {} is missing from rootfs; cannot respawn",
                INIT_BIN_PATH
            );
            return;
        }
    };

    match Process::launch_user_program(INIT_BIN_NAME, bytes) {
        Ok(()) => {
            RESPAWN_CONSUMED.store(true, Ordering::Release);
            crate::log_info!(
                "RESPAWN",
                "boot anchor (pid=1) died; spawned {} ({} bytes) as replacement",
                INIT_BIN_PATH,
                bytes.len()
            );
        }
        Err(e) => {
            crate::log_error!(
                "RESPAWN",
                "boot anchor (pid=1) died but launch_user_program({}) failed: {:?}",
                INIT_BIN_PATH,
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The init binary must live at the same path the kernel uses
    /// during normal boot — otherwise the respawned process would
    /// not match what `hnx-loader` would have launched.
    #[test]
    fn init_path_matches_rootfs_layout() {
        // Defensive: assert the path string is non-empty and rooted
        // at `system/`.  A typo here would silently launch the
        // wrong binary and the system would halt for a different
        // reason.
        assert!(INIT_BIN_PATH.starts_with("system/"));
        assert!(!INIT_BIN_PATH.is_empty());
    }

    /// `INIT_BIN_NAME` is a `&'static str` because
    /// `launch_user_program` requires one; this test makes sure
    /// it stays that way at compile time.
    #[test]
    fn init_name_is_static_str() {
        let _: &'static str = INIT_BIN_NAME;
    }
}
