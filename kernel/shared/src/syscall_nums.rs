//! # SYSCALL_* 编号总表 —— kernel / userspace 单一真理源
//!
//! 历史上 `kernel/src/syscall/numbers.rs` 与 `hnxlibc/src/syscalls.rs`
//! 各自维护了一份相同的常量表。本文件承担**两边共享**的角色，
//! 与 `Status`、`HandleValue` 等已被 `kernel/shared` 收纳的
//! ABI 契约项并列。kernel 自身也改为 `pub use
//! shared::syscall_nums::*` —— 因此全仓再不存在第二份 `SYSCALL_*`
//! 数值定义。
//!
//! ## 编号空间布局
//!
//! | 范围      | 类别                          | 状态   |
//! |-----------|-------------------------------|--------|
//! | 0-3       | POSIX 基础 (退/写/取 tid/pid) | 实装    |
//! | 10-16     | IPC Channel + 句柄复制         | 实装    |
//! | 13        | CHANNEL_CALL                  | 预留    |
//! | 20-22     | IPC Port                      | 预留    |
//! | 30-34     | VMO (含 GET/SET_SIZE)         | 实装    |
//! | 40-42     | VMAR (UNMAP/PROTECT 预留)      | 实装    |
//! | 50-52     | Thread (EXIT 预留)            | 实装    |
//! | 60-62     | Process (START/EXIT 预留)     | 实装    |
//! | 70-72     | Event                         | 预留    |
//! | 80-82     | Timer                         | 预留    |
//! | 90-91     | Futex                         | 预留    |
//! | 100-105   | POSIX fd 集 (含 SEEK)         | 实装    |
//! | 110-114   | ELF/EXEC/SPAWN/YIELD          | 实装    |
//!
//! 实装 arm 仍在 dispatcher 表中；预留 arm 仍受 `SYSCALL_NR` sentinel
//! 守护，调用侧会得到 `Status::NotAllowed`，参见 `syscall_dispatch`。

pub const SYSCALL_EXIT: u32 = 0;
pub const SYSCALL_WRITE: u32 = 1;
pub const SYSCALL_GET_TID: u32 = 2;
pub const SYSCALL_GET_PID: u32 = 3;

pub const SYSCALL_CHANNEL_CREATE: u32 = 10;
pub const SYSCALL_CHANNEL_READ: u32 = 11;
pub const SYSCALL_CHANNEL_WRITE: u32 = 12;
pub const SYSCALL_CHANNEL_CALL: u32 = 13;
pub const SYSCALL_CHANNEL_REGISTER: u32 = 14;
pub const SYSCALL_CHANNEL_LOOKUP: u32 = 15;
pub const SYSCALL_HANDLE_DUPLICATE: u32 = 16;
pub const SYSCALL_PORT_CREATE: u32 = 20;
pub const SYSCALL_PORT_WAIT: u32 = 21;
pub const SYSCALL_PORT_QUEUE: u32 = 22;

pub const SYSCALL_VMO_CREATE: u32 = 30;
pub const SYSCALL_VMO_READ: u32 = 31;
pub const SYSCALL_VMO_WRITE: u32 = 32;
pub const SYSCALL_VMO_GET_SIZE: u32 = 33;
pub const SYSCALL_VMO_SET_SIZE: u32 = 34;
pub const SYSCALL_VMO_CREATE_CHILD: u32 = 35;

pub const SYSCALL_VMAR_MAP: u32 = 40;
pub const SYSCALL_VMAR_UNMAP: u32 = 41;
pub const SYSCALL_VMAR_PROTECT: u32 = 42;

pub const SYSCALL_THREAD_CREATE: u32 = 50;
pub const SYSCALL_THREAD_START: u32 = 51;
pub const SYSCALL_THREAD_EXIT: u32 = 52;

pub const SYSCALL_PROCESS_CREATE: u32 = 60;
pub const SYSCALL_PROCESS_START: u32 = 61;
pub const SYSCALL_PROCESS_EXIT: u32 = 62;

pub const SYSCALL_EVENT_CREATE: u32 = 70;
pub const SYSCALL_EVENT_SIGNAL: u32 = 71;
pub const SYSCALL_EVENT_ACK: u32 = 72;

pub const SYSCALL_TIMER_CREATE: u32 = 80;
pub const SYSCALL_TIMER_SET: u32 = 81;
pub const SYSCALL_TIMER_CANCEL: u32 = 82;

pub const SYSCALL_FUTEX_WAIT: u32 = 90;
pub const SYSCALL_FUTEX_WAKE: u32 = 91;

pub const SYSCALL_OPEN: u32 = 100;
pub const SYSCALL_CLOSE: u32 = 101;
pub const SYSCALL_READ: u32 = 102;
pub const SYSCALL_SEEK: u32 = 103;
pub const SYSCALL_GETCWD: u32 = 104;
pub const SYSCALL_CHDIR: u32 = 105;

/// POSIX `pipe(ufds[2])` - allocate a fresh in-kernel pipe and
/// place its two fd numbers in the caller's per-process fd table
/// at indices `ufds[0]` (read end) and `ufds[1]` (write end).
/// `ufds_ptr` is a user VA pointing to two consecutive `i32`
/// slots; the kernel writes the new fd numbers back via
/// `safe_copy_to_user`.  Returns 0 on success; `NoMemory` if
/// the pipe table or the calling process's fd table is full.
pub const SYSCALL_PIPE: u32 = 96;

/// POSIX `dup2(oldfd, newfd)` - duplicate `oldfd` into `newfd`.
/// For 1.0 we only support the common "redirect stdout to a
/// pipe's write end" pattern from shell pipelines:
///   newfd == 1 -> dup into the in-kernel UART stdout path
///                (treated as a no-op + NotAllowed because the
///                kernel always owns fd 1)
///   newfd >= 3 -> if oldfd is a pipe-end, the new fd in the
///                caller's fd_table gets the same `PipeRole`.
///                If `newfd == oldfd` the syscall is a no-op
///                success (matching Linux).
/// Returns the new fd (which equals `newfd` on success) or a
/// Status code.
pub const SYSCALL_DUP2: u32 = 97;

/// Wait for a child process to exit, optionally restricted to a
/// specific pid.  `pid` semantics match Linux's `wait4(pid, ...)`:
///
///   pid > 0   - wait for child whose process_id == pid
///   pid = 0   - wait for any child whose parent is the caller and
///                whose process group is the caller's pgrp
///                (process-group support is a stub for 1.0; we
///                collapse to "wait for any direct child")
///   pid = -1  - wait for any child of the caller (1.0 behaviour)
///   pid < -1  - process-group wait (1.0 stub, returns InvalidArgs)
///
/// On success the user-mode buffer pointed to by `status_out_ptr`
/// receives a Posix-style exit status (low 8 bits = exit code; bit
/// 8 set = killed by signal; we only distinguish "WIFEXITED" + the
/// raw 8-bit code in 1.0 -- signal exit is reserved for the B5
/// signal pipeline).  Returns the pid of the reaped child, or
/// `Status::TryAgain` if the chosen child is still running, or
/// `Status::NotFound` if no such child exists.
pub const SYSCALL_WAIT4: u32 = 88;

// -------------------------------------------------------------------------
// B5: POSIX `signal.h` surface
//
// We allocate the 92-95 range for the small signal/raise/kill/sigprocmask
// family that drives the 1.0 demo: shell pipelines (B7) need to be able
// to terminate children, ignore SIGPIPE on broken pipes, and propagate
// Ctrl-C across the foreground process group.  We expose:
//   - sigaction  (degenerate: SIG_DFL / SIG_IGN only)
//   - raise      (self-targeted)
//   - kill       (cross-process)
//   - pause      (yield until any non-blocked signal is pending)
//
// sigprocmask is a 1.0 stub returning InvalidArgs because we have no
// per-process signal-block table yet; B7's pipeline uses the SIG_DFL /
// SIG_IGN only through sigaction.
// -------------------------------------------------------------------------

/// Install (or query) the disposition for a signal.
///
/// act.sa_handler is a `usize` value; we accept:
///   0 (SIG_DFL) - default disposition (kill the process for most sigs)
///   1 (SIG_IGN) - ignore the signal
/// We do not support user-mode handler trampolines in 1.0 (those
/// would require an SA_RESTORER-style sigreturn frame and an aarch64
/// trampoline in hnxlibc).  Returns the *previous* disposition's
/// `sa_handler` value so a caller can chain.
pub const SYSCALL_SIGACTION: u32 = 92;

/// Self-targeted signal: send `signo` to the calling process.
pub const SYSCALL_RAISE: u32 = 93;

/// Cross-process signal: send `signo` to process `pid`.  `pid`
/// follows the kill(2) convention but for 1.0 only `pid > 0`
/// (specific process) and `pid = 0` (any child via the wait4
/// path) are wired; `pid < -1` / `pid = -1` return InvalidArgs.
pub const SYSCALL_KILL: u32 = 94;

/// Suspend the calling thread until a non-blocked, non-ignored
/// signal becomes pending.  Always returns `Status::Ok` once the
/// caller is resumed (the signal that woke us has already been
/// processed by the kernel-side dispatcher on the way out).
pub const SYSCALL_PAUSE: u32 = 95;

pub const SYSCALL_EXEC: u32 = 110;
pub const SYSCALL_LOAD_BINARY: u32 = 111;
pub const SYSCALL_EXECVE: u32 = 112;
/// Spawn an EL0 process from the embedded rootfs by path (short name or
/// absolute `"system/bin/..."` path).  Unlike `SYSCALL_EXEC`, this does
/// **not** replace the caller — the new process is added alongside, and
/// control returns to the caller immediately.  Returns the new pid
/// (> 0) on success or a negative `Status::to_raw()` on failure.
pub const SYSCALL_SPAWN: u32 = 113;
/// Voluntarily relinquish the CPU until the next timer tick or higher
/// priority event.  Used by EL0 services (loader / init) that need to
/// give the scheduler a chance to run a freshly-spawned process (fileagent
/// registering `svc.vfs`, etc.) without blocking on the IPC bus.
pub const SYSCALL_YIELD: u32 = 114;

/// One past the last valid syscall number.  Any `syscall_num >= SYSCALL_NR`
/// is reserved by the ABI for future extensions and must not be accepted by
/// the dispatcher — see `kernel/src/syscall/mod.rs`.
pub const SYSCALL_NR: u32 = 115;
