# 📁 `kernel::task::signals` — POSIX-style Signal Model (1.0 Subset)

## Status
[Active (使用中)]

## Name
**`kernel::task::signals`** — minimal POSIX signal model that supports
`SIG_DFL` (terminate) and `SIG_IGN` (drop) dispositions, `raise` /
`kill` / `pause`, and `wait4` reaping on default-disposition termination.

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `kernel::task::process` | Stores `pending_signals: u32` (bitfield, 1 bit per signal number 1..=32) and `sig_handlers: [u8; 32]` on the `Process` struct.  All signal state reads/writes go through `find_process_mut(pid)`. |
| `kernel::syscall::handlers::process::{sys_sigaction, sys_raise, sys_kill, sys_pause}` | Syscall entry points that delegate to this module.  `sys_wait4` also fires here when a child lands in `Zombie` from a SIG_DFL reaping. |
| `kernel::arch::aarch64` | `dispatch_pending` returns `Ok(true)` to indicate the scheduler must drop the dispatching thread; the helper uses the standard `current_thread_ptr` swap path. |
| `libc::syscalls::{sigaction, raise, kill, pause}` | C-ABI wrappers that the user-side employs.  Per DEVELOPMENT.md §5, `sigaction` rejects custom user-mode trampolines at the libc boundary with `EINVAL`. |

## Core Definition

### Why this exists

CapsuleOS 1.0 has 32 signals (`NSIG = 32`) packed into a single `u32`
bitfield on the `Process` struct — bit `n - 1` represents signal `n`.
This keeps the dispatch state in a single cache line and avoids
dynamic allocation on the hot path (`process.pending_signals` is
copied into a local for the bit-walk).

### Scope of the 1.0 implementation

* `sigaction(sig, handler)` — accepts `SIG_DFL (0)` and `SIG_IGN (1)`
  only.  Custom user-mode trampolines are rejected at
  `task::signals::sigaction_set` with `Status::InvalidArgs`.  Custom
  handlers would require a `sigreturn` frame setup (saving/restoring
  user-mode registers around a pre-arranged trampoline) and that
  ticket is open in `KERNEL_HEALTH.md` (future B5.1).
* `raise(sig)` — self-targeted signal: sets `pending_signals |= bit`.
* `kill(pid, sig)` — cross-process signal.  1.0 rule: a process may
  signal itself or any of its direct children.  `pid = 0` broadcasts
  to all direct children.  `pid < 0` is reserved for future
  process-group support.
* `dispatch_pending(pid)` — walks the bitfield at syscall exit, picks
  the lowest-numbered non-ignored, non-blocked signal, and either
  terminates the process (SIG_DFL) or drops the bit (SIG_IGN).
* `pause()` — `SYSCALL_PAUSE` yields the calling thread via
  `thread_sleep` until a non-blocked signal is pending.

### Bit-field storage

`Process::pending_signals` is `u32`, `Process::sig_handlers` is
`[u8; 32]`.  We map `0 → bit 0`, `1 → bit 1`, …, `31 → bit 31`.  The
dispatcher iterates bits 0..NSIG in order so signals are
deterministic — `bash`'s `trap` infrastructure can rely on FIFO
delivery order for the same disposition class.

### Why a separate file from `process.rs`

The signal state-machine is non-trivial: bit-field iteration +
process state transitions + threading lock domain cross in.  Keeping
it isolated leaves `process.rs` for the lifetime model and `thread.rs`
for the run queue.

## Exposed Interfaces

### Constants

| Symbol | Value | Purpose |
|--------|-------|---------|
| `SIG_DFL` | `0` | POSIX default disposition (terminate on delivery). |
| `SIG_IGN` | `1` | POSIX ignore disposition (drop on delivery). |
| `NSIG` | `32` | Maximum signal number + 1.  Private to the kernel. |

### Functions

| Symbol | Purpose |
|--------|---------|
| `fn sigaction_set(sig: usize, handler: usize) -> Result<usize>` | Set the disposition for `sig`; returns the previous disposition so callers can chain.  Rejects `handler` outside `{SIG_DFL, SIG_IGN}` with `InvalidArgs`. |
| `fn signal_send(target_pid: u64, sig: usize) -> Result<()>` | Queue `sig` for `target_pid`.  Returns `NotFound` if `target_pid` is empty. |
| `fn raise(sig: usize) -> Result<()>` | Self-targeted signal.  Helper around `signal_send(current_pid, sig)`. |
| `fn dispatch_pending(caller_pid: u64) -> Result<bool>` | Called from the syscall-exit path.  Returns `Ok(true)` if the caller must yield the CPU (signal consumed), `Ok(false)` otherwise. |

### Vended to syscall layer

| Symbol | Path |
|--------|------|
| `sys_sigaction(sa_handler, mask, flags) -> Result<usize>` | `kernel/src/syscall/handlers/process.rs:1438` |
| `sys_raise(sig) -> Result<()>` | `process.rs:1449` |
| `sys_kill(pid, sig) -> Result<()>` | `process.rs:1471` |
| `sys_pause() -> Result<()>` | `process.rs:1509` (uses `thread_sleep`, bounded ~70s) |

## Invariants / Caveats

1. **No SIGCONT/SIGSTOP infrastructure** — `wait4`'s `WUNTRACED=2`
   flag is accepted as a no-op for ABI compatibility but does not
   transition the process to a `Stopped` state.  When SIGCONT/SIGSTOP
   land (B5.1), the bit-walk will need to consult a `sig_chains` table
   rather than just the bitfield.
2. **Single-threaded process** — `pending_signals` is read-modify-write
   against the calling process only.  No inter-processor synchronisation
   is required because `kill(pid, sig)` runs on the same CPU as the
   kill-receiving caller (1.0 has no remote-poke queue yet).
3. **`SIG_DFL` is terminal** — a single `SIG_DFL`-dispositioned signal
   in `pending_signals` collapses the process to `Zombie` with
   `exit_status = Some(-1)` (the `SIG_DFL_KILL_EXIT` sentinel).  The
   parent `wait4` sees a negative `exit_status` and uses `WIFSIGNALED`
   to distinguish a signal kill from a clean exit.
4. **`SIG_IGN` is silent** — `SIG_IGN` bits are dropped without
   informing the dispatcher.  The next non-IGN bit is then considered.
5. **Block mask is always zero** — `sigprocmask` is a stub in 1.0 so
   the walk never has to filter on a per-thread mask.  Once the
   thread structure is rich enough to support per-thread masking, the
   walk will need to filter against `current_thread.blocked_signals`.
6. **No `sigpending` / `sigsuspend`** — both are `ENOSYS` and the
   bitfield is queried indirectly via `kill(0, sig)` returning the
   count.

## Future Work

* **B5.1: custom user-mode handlers** — register a function pointer
  in `sig_handlers`, set up a `sigreturn` frame on the user stack,
  carry registers to the user-mode trampoline, and `eret` back into
  the kernel only when the user-mode handler returns.  This unlocks
  bash's `trap "echo got USR1" USR1` in a 1.1 release.
* **B5.2: per-thread signal mask** — promote `sigprocmask` to a real
  implementation.  Storage growth: 32-bit per thread × 5 threads ≈
  160 bytes process-wide.  `dispatch_pending` gains a `masked = false`
  filter.
* **B5.3: SIGSTOP/SIGTSTP** — `Stop` process state, `SIGCONT` resume,
  `wait4` `WUNTRACED` reporting.  Requires a new `ProcessState::Stopped`
  variant and a `Stopped → Ready` edge in the scheduler.
* **B5.4: real-time signals** — `SIGRTMIN..SIGRTMAX` (33..64) on top
  of the existing 32-bit field, codepoint extension to a `[u64; 2]`
  backing store.  Out of scope for 1.0.

## Cross-References

* `kernel/src/syscall/handlers/process.rs:1438-1507` — `sys_sigaction`,
  `sys_raise`, `sys_kill`, `sys_pause` entry points.
* `kernel/src/task/process.rs` — `Process::pending_signals`,
  `Process::sig_handlers`, `Process::exit_status`, `Process::state`.
* `KERNEL_HEALTH.md` B5 — the bucket that opened this module.
* Tier B S6 (`6960835`) — `sys_wait4` returns Ok(0) on `WNOHANG`
  for programs that probe a child without blocking.
