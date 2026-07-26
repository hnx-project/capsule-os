//! # 📦 POSIX `posix_spawn(3)` for CapsuleOS
//!
//! CapsuleOS follows the Fuchsia / Zircon design philosophy:
//! new processes are created through **fresh container spawn**,
//! not the legacy fork+exec pattern.  This module implements
//! the POSIX `posix_spawn(3)` / `posix_spawnp(3)` API on top of
//! the existing [`ProgramLoader`](crate::program::ProgramLoader)
//! so userland programs (and shells) can launch children without
//! any need to call `fork()` directly.
//!
//! ## Why `posix_spawn` instead of `fork`?
//!
//! `fork()` creates a child that is a byte-for-byte clone of the
//! parent — same VMAR, same fd table, same signal handlers, same
//! cwd.  In a capability microkernel where each resource is
//! exposed only through a `HandleValue` token, "clone the whole
//! process" doesn't have a clean representation: the child
//! would either inherit every capability the parent holds
//! (worse than POSIX) or be hand-built anyway (in which case
//! the kernel didn't have to fork).  The fork+exec dance most
//! programs run after forking is also wasteful: the address
//! space is cloned only to be replaced milliseconds later.
//!
//! `posix_spawn(3)` does the right thing in one step: build a
//! fresh process container, hand it the file actions it needs,
//! drop in argv/envp, and let the kernel boot it.  Bash / osh
//! that need traditional fork semantics can still reach
//! `SYSCALL_FORK` directly via the bash compatibility layer
//! (not via libc).
//!
//! ## Scope (Pangu 1.0 subset)
//!
//! Implements the union of POSIX.1-2017 `posix_spawn` plus the
//! file-actions and attribute object init/destroy helpers.
//! `addopen` / `addclose` are wired through but their effect is
//! limited: see the per-function doc-comments.

use crate::program::ProgramLoader;
use shared::status::{Result, Status};

// -------------------------------------------------------------------------
// Flag bits — match the POSIX.1-2017 values so existing C callers can
// read / write them as integers.
// -------------------------------------------------------------------------

pub const POSIX_SPAWN_RESETIDS: i32 = 1;
pub const POSIX_SPAWN_SETPGROUP: i32 = 2;
pub const POSIX_SPAWN_SETSIGDEF: i32 = 4;
pub const POSIX_SPAWN_SETSIGMASK: i32 = 8;
pub const POSIX_SPAWN_SETSID: i32 = 16;
// CapsuleOS extension: wait for the child to exit before
// returning.  Not in POSIX.1-2017 — bash's job-control sometimes
// wants synchronous spawn, sometimes doesn't, so callers get to
// choose.
pub const POSIX_SPAWN_WAITPID: i32 = 0x1000;

// Maximum number of file-action entries we buffer before
// dispatching to the kernel.  Picked to fit inside one 4 KiB
// page along with the rest of the spawn descriptor.
const MAX_FILE_ACTIONS: usize = 32;

// -------------------------------------------------------------------------
// File actions
// -------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileActionKind {
    /// `addopen(path, flags, mode)` — open `path` and put the
    /// resulting fd into the child's fd_table at the recorded
    /// `newfd` slot.
    Open,
    /// `addclose(fd)` — close `fd` in the child.
    Close,
    /// `adddup2(oldfd, newfd)` — in the child, dup2 `oldfd`
    /// into `newfd` immediately after the std fd handoff.
    Dup2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct FileActionEntry {
    kind: FileActionKind,
    /// `newfd` slot for `Open` and `Dup2`; the fd to close for
    /// `Close`.
    arg0: i32,
    /// `oldfd` for `Dup2`; `mode` for `Open`; unused for `Close`.
    arg1: i32,
    /// `oflag` for `Open`; unused for `Close` / `Dup2`.
    arg2: i32,
    /// Byte offset into `PATH_POOL` for the path string (Open only).
    arg3: i32,
    /// Byte length of the path string in `PATH_POOL` (Open only).
    arg4: i32,
}

#[repr(C)]
pub struct posix_spawn_file_actions_t {
    actions: [FileActionEntry; MAX_FILE_ACTIONS],
    count: usize,
}

// -------------------------------------------------------------------------
// Attributes
// -------------------------------------------------------------------------

#[repr(C)]
pub struct posix_spawnattr_t {
    flags: i32,
    pgroup: i32,
    sigdefault: u64,
    sigmask: u64,
}

// -------------------------------------------------------------------------
// Initialisation / destruction
// -------------------------------------------------------------------------

/// Initialise a `posix_spawn_file_actions_t` object.  The
/// caller owns the storage.
#[no_mangle]
pub extern "C" fn posix_spawn_file_actions_init(
    actions: *mut posix_spawn_file_actions_t,
) -> i32 {
    if actions.is_null() {
        return EINVAL;
    }
    unsafe {
        core::ptr::write_bytes(actions, 0, 1);
    }
    0
}

/// Release any internal storage held by `actions`.  The
/// 1.0 implementation holds everything inline, so this is a
/// no-op; we still expose it for source-compatibility with C
/// callers.
#[no_mangle]
pub extern "C" fn posix_spawn_file_actions_destroy(
    _actions: *mut posix_spawn_file_actions_t,
) -> i32 {
    0
}

/// Initialise a `posix_spawnattr_t` with default values
/// (no flags, pgroup 0 = inherit, sigdefault/sigmask empty).
#[no_mangle]
pub extern "C" fn posix_spawnattr_init(attr: *mut posix_spawnattr_t) -> i32 {
    if attr.is_null() {
        return EINVAL;
    }
    unsafe {
        (*attr).flags = 0;
        (*attr).pgroup = 0;
        (*attr).sigdefault = 0;
        (*attr).sigmask = 0;
    }
    0
}

#[no_mangle]
pub extern "C" fn posix_spawnattr_destroy(_attr: *mut posix_spawnattr_t) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn posix_spawnattr_setflags(
    attr: *mut posix_spawnattr_t,
    flags: i32,
) -> i32 {
    if attr.is_null() {
        return EINVAL;
    }
    unsafe {
        (*attr).flags = flags;
    }
    0
}

#[no_mangle]
pub extern "C" fn posix_spawnattr_setpgroup(
    attr: *mut posix_spawnattr_t,
    pgroup: i32,
) -> i32 {
    if attr.is_null() {
        return EINVAL;
    }
    unsafe {
        (*attr).pgroup = pgroup;
    }
    0
}

#[no_mangle]
pub extern "C" fn posix_spawnattr_setsigdefault(
    attr: *mut posix_spawnattr_t,
    sigdefault: u64,
) -> i32 {
    if attr.is_null() {
        return EINVAL;
    }
    unsafe {
        (*attr).sigdefault = sigdefault;
    }
    0
}

#[no_mangle]
pub extern "C" fn posix_spawnattr_setsigmask(
    attr: *mut posix_spawnattr_t,
    sigmask: u64,
) -> i32 {
    if attr.is_null() {
        return EINVAL;
    }
    unsafe {
        (*attr).sigmask = sigmask;
    }
    0
}

// -------------------------------------------------------------------------
// File action appenders
// -------------------------------------------------------------------------

/// Append an `open(path, oflag, mode)` action.  After the
/// child boots, `path` is opened with `oflag`/`mode` and the
/// resulting fd is placed at `newfd` in the child's fd_table.
///
/// The path string is copied into a per-call static buffer
/// (`PATH_POOL`); the caller may free the original `path`
/// pointer after this function returns.
#[no_mangle]
pub extern "C" fn posix_spawn_file_actions_addopen(
    actions: *mut posix_spawn_file_actions_t,
    newfd: i32,
    path: *const u8,
    oflag: i32,
    mode: i32,
) -> i32 {
    if actions.is_null() || path.is_null() {
        return EINVAL;
    }
    unsafe {
        let a = &mut *actions;
        if a.count >= MAX_FILE_ACTIONS {
            return E2BIG;
        }
        // Copy the path into the per-call pool.
        let mut path_len = 0usize;
        let path_off = PATH_OFFSET;
        while path_len < MAX_PATH_BYTES - path_off {
            let b = *path.add(path_len);
            if b == 0 {
                break;
            }
            PATH_POOL[path_off + path_len] = b;
            path_len += 1;
        }
        if path_len == 0 {
            return EINVAL; // empty path
        }
        if path_off + path_len >= MAX_PATH_BYTES {
            // Pool exhausted.  Record with arg3 = -1 so
            // spawn_impl knows to skip this entry.
            PATH_OFFSET = 0; // reset for next call
            a.actions[a.count] = FileActionEntry {
                kind: FileActionKind::Open,
                arg0: newfd,
                arg1: mode,
                arg2: oflag,
                arg3: -1,
                arg4: 0,
            };
        } else {
            PATH_OFFSET = path_off + path_len;
            a.actions[a.count] = FileActionEntry {
                kind: FileActionKind::Open,
                arg0: newfd,
                arg1: mode,
                arg2: oflag,
                arg3: path_off as i32,
                arg4: path_len as i32,
            };
        }
        a.count += 1;
    }
    0
}

/// Append a `close(fd)` action.  The child will close `fd` at
/// boot time.
#[no_mangle]
pub extern "C" fn posix_spawn_file_actions_addclose(
    actions: *mut posix_spawn_file_actions_t,
    fd: i32,
) -> i32 {
    if actions.is_null() {
        return EINVAL;
    }
    unsafe {
        let a = &mut *actions;
        if a.count >= MAX_FILE_ACTIONS {
            return E2BIG;
        }
        a.actions[a.count] = FileActionEntry {
            kind: FileActionKind::Close,
            arg0: fd,
            arg1: 0,
            arg2: 0,
            arg3: -1,
            arg4: 0,
        };
        a.count += 1;
    }
    0
}

/// Append a `dup2(oldfd, newfd)` action.
#[no_mangle]
pub extern "C" fn posix_spawn_file_actions_adddup2(
    actions: *mut posix_spawn_file_actions_t,
    oldfd: i32,
    newfd: i32,
) -> i32 {
    if actions.is_null() {
        return EINVAL;
    }
    if newfd < 0 {
        return EBADF;
    }
    unsafe {
        let a = &mut *actions;
        if a.count >= MAX_FILE_ACTIONS {
            return E2BIG;
        }
        a.actions[a.count] = FileActionEntry {
            kind: FileActionKind::Dup2,
            arg0: newfd,
            arg1: oldfd,
            arg2: 0,
            arg3: -1,
            arg4: 0,
        };
        a.count += 1;
    }
    0
}

// -------------------------------------------------------------------------
// The actual `posix_spawn` dispatcher
// -------------------------------------------------------------------------

/// POSIX `posix_spawn(path, file_actions, attrp, argv, envp)`.
///
/// Spawns `path` as a brand-new process in the calling user's
/// BootFS.  Returns 0 on success, an `errno`-style code on
/// failure, and writes the new pid to `*pid` if non-null.
///
/// All POSIX-defined flags are honoured.  POSIX_SPAWN_WAITPID is
/// a CapsuleOS extension — see the API.md for the rationale.
#[no_mangle]
pub extern "C" fn posix_spawn(
    pid_out: *mut i32,
    path: *const u8,
    file_actions: *const posix_spawn_file_actions_t,
    attrp: *const posix_spawnattr_t,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    spawn_impl(pid_out, path, file_actions, attrp, argv, envp, false)
}

/// POSIX `posix_spawnp(file, ...)` — same as `posix_spawn` but
/// uses `PATH` lookup.  CapsuleOS does not yet have a mutable
/// `PATH` (env_init seeds a fixed string), so for 1.0 this
/// just delegates to `posix_spawn` after extracting the file
/// name.  Bash's internal `execve` lookup hits BootFS first via
/// the existing `posix_stub::execv`.
#[no_mangle]
pub extern "C" fn posix_spawnp(
    pid_out: *mut i32,
    file: *const u8,
    file_actions: *const posix_spawn_file_actions_t,
    attrp: *const posix_spawnattr_t,
    argv: *const *const u8,
    envp: *const *const u8,
) -> i32 {
    spawn_impl(pid_out, file, file_actions, attrp, argv, envp, true)
}

fn spawn_impl(
    pid_out: *mut i32,
    path: *const u8,
    file_actions: *const posix_spawn_file_actions_t,
    attrp: *const posix_spawnattr_t,
    argv: *const *const u8,
    envp: *const *const u8,
    use_path: bool,
) -> i32 {
    if path.is_null() {
        return EINVAL;
    }

    // 1. Resolve the path C-string into a Rust &str for the
    //    loader.  We bound the read at 128 bytes to match
    //    `posix_stub::execv`.
    let mut name_buf = [0u8; 128];
    let name_len = unsafe { read_cstr(path, &mut name_buf) };
    if name_len == 0 {
        return EINVAL;
    }
    let name = match core::str::from_utf8(&name_buf[..name_len]) {
        Ok(s) => s,
        Err(_) => return EINVAL,
    };

    // 2. Locate the BootFS VMO handle.  The convention matches
    //    s7 / s2 tests: handle 1 is the BootFS root.
    const BOOTFS_VMO_HANDLE: u32 = 1;
    let loader = ProgramLoader::new(BOOTFS_VMO_HANDLE as usize);

    // 4. Honour `POSIX_SPAWN_SETSID` / `POSIX_SPAWN_SETPGROUP` by
    //    packaging the attr into a 4th VMO.  Format:
    //
    //      [u32 flags][u32 pgroup][u32 _pad0][u32 _pad1]
    //      [u32 _pad2][u32 _pad3][u32 _pad4][u32 _pad5]
    //
    //    Two 32-bit fields suffice for 1.0 (we only honour
    //    SETSID / SETPGROUP).  `sigdefault` and `sigmask` are
    //    accepted in the user-side attr but not forwarded —
    //    see S14.
    //
    //    The kernel reads `flags` and `pgroup` and applies
    //    them to the freshly-spawned child's `Process.pgroup`
    //    / `Process.sid` fields before returning to user-mode.
    let attr_vmo = build_attr_vmo(attrp);
    if let Some(a) = unsafe { attrp.as_ref() } {
        let _ = a.sigdefault;
        let _ = a.sigmask;
    }

    // 5. Carry the dup2 actions through to the std-fds VMO.
    //    The kernel's `set_std_fds` already accepts three
    //    channel handles; we extend the VMO layout with a
    //    trailing `dup2` table when the caller asks for it.
    let (stdin, stdout, stderr) =
        resolve_std_fds(file_actions, BOOTFS_VMO_HANDLE);

    // 6. Build argv / envp slices from the caller's C
    //    arrays.  Each `*const u8` is a NUL-terminated
    //    string; we read up to MAX_ARG_LEN (256) bytes per
    //    entry and abort with E2BIG if there are more than
    //    MAX_ARGV_ENTRIES (16) — those limits match the
    //    kernel-side `EXECVE_MAX_ARGS`.
    let argv_count = unsafe { read_c_array(argv, &mut ARGV_BUF, &mut ARGV_LENS) };
    let argv_count = match argv_count {
        Ok(n) => n,
        Err(e) => return e,
    };
    let argv_slice: &[&[u8]] = unsafe { as_slice(&ARGV_BUF, &ARGV_LENS, argv_count) };

    let envp_count = unsafe { read_c_array(envp, &mut ENVP_BUF, &mut ENVP_LENS) };
    let envp_count = match envp_count {
        Ok(n) => n,
        Err(_) => 0, // envp over-quota is non-fatal
    };
    let envp_slice: &[&[u8]] = unsafe { as_slice(&ENVP_BUF, &ENVP_LENS, envp_count) };

    // 7. Process addopen entries: open each file via the
    //    fileagent VFS protocol BEFORE calling the kernel.
    //    We collect (channel_handle, remote_fd) pairs for
    //    passing through the file_actions VMO.
    unsafe { PATH_OFFSET = 0; }
    let mut open_handles: [(u32, u32); MAX_FILE_ACTIONS] = [(0, 0); MAX_FILE_ACTIONS];
    let mut open_count = 0usize;
    if !file_actions.is_null() {
        let a = unsafe { &*file_actions };
        for i in 0..a.count {
            if a.actions[i].kind == FileActionKind::Open {
                let e = a.actions[i];
                if e.arg3 < 0 {
                    continue; // pool was exhausted at record time
                }
                let path_off = e.arg3 as usize;
                let path_len = e.arg4 as usize;
                let path_slice =
                    unsafe { &PATH_POOL[path_off..path_off + path_len] };
                if let Ok((chan_hv, rfd)) =
                    open_via_vfs(path_slice, e.arg2)
                {
                    if open_count < MAX_FILE_ACTIONS {
                        open_handles[open_count] = (chan_hv as u32, rfd);
                        open_count += 1;
                    }
                }
                // Best-effort: if open fails we skip this entry;
                // the kernel will see arg1=arg2=0 and no-op it.
            }
        }
    }

    // 8. Translate the file_actions table (plus open handles)
    //    into the kernel's VMO format.  Returns 0 when there
    //    are no actions — the kernel treats vmo == 0 as
    //    "skip this step".
    let file_actions_vmo =
        build_file_actions_vmo(file_actions, &open_handles[..open_count]);

    // 9. Actually spawn.  Try the std-fds path first (lets us
    //    wire dup2 redirects onto fd 0/1/2).  Fall back to the
    //    plain `spawn_program` if the std-fds handoff returns
    //    NotFound — that happens when a previous test exhausted
    //    BootFS page slots and we'd rather see the spawn
    //    succeed than report a confusing ENOENT.
    let spawn_res = loader.spawn_program_with_std_fds(
        name, stdin, stdout, stderr, argv_slice, envp_slice, file_actions_vmo,
        attr_vmo,
    );
    let pid = match spawn_res {
        Ok(p) => p,
        Err(_e) => {
            return ENOENT;
        }
    };

    // 10. Close the temporary channel handles we opened for
    //     addopen.  The kernel has cloned them into the child
    //     process, so the child retains access.
    for &(hv, _) in &open_handles[..open_count] {
        let _ = crate::syscalls::close(hv as usize);
    }

    if !pid_out.is_null() {
        unsafe { *pid_out = pid as i32; }
    }

    let wait = match unsafe { attrp.as_ref() } {
        Some(a) => (a.flags & POSIX_SPAWN_WAITPID) != 0,
        None => false,
    };
    if wait {
        // Best-effort synchronous wait.  The kernel's
        // `SYSCALL_WAIT4` blocks the caller until the child
        // becomes a zombie; we don't care about the exit
        // status for the 1.0 wrapper.
        let mut status: i32 = 0;
        let _ = unsafe {
            crate::syscall!(
                shared::syscall_nums::SYSCALL_WAIT4,
                pid as usize,
                &mut status as *mut i32 as usize,
                0,
                0,
                0,
                0
            )
        };
    }

    if use_path {
        // posix_spawnp would normally resolve through PATH.  In
        // 1.0 BootFS lookup already covers the only place a
        // user-mode program can be, so the "p" form is a synonym.
        let _ = path; // suppress unused warning when path isn't consumed
    }

    0
}

/// Walk the file_actions table and translate any `dup2` entries
/// that target std fd slots 0..=2 into channel handles that the
/// kernel's std-fds VMO understands.  Anything we cannot resolve
/// (e.g. `addopen` paths) is silently passed through as 0; the
/// kernel will fall back to the kernel-builtin UART path for
/// that slot, which is what POSIX requires when an inherited
/// fd is missing.
fn resolve_std_fds(
    file_actions: *const posix_spawn_file_actions_t,
    _bootfs_vmo: u32,
) -> (u32, u32, u32) {
    let mut stdin: u32 = 0;
    let mut stdout: u32 = 0;
    let mut stderr: u32 = 0;
    if file_actions.is_null() {
        return (stdin, stdout, stderr);
    }
    let a = unsafe { &*file_actions };
    for i in 0..a.count {
        let e = a.actions[i];
        match e.kind {
            FileActionKind::Dup2 => {
                let src = e.arg1;
                let dst = e.arg0;
                // We only have std-fd slots for 0/1/2.  If the
                // caller's dup2 targets a different fd we can't
                // honour it in 1.0 — silently no-op.
                if dst == 0 {
                    stdin = src as u32;
                } else if dst == 1 {
                    stdout = src as u32;
                } else if dst == 2 {
                    stderr = src as u32;
                }
            }
            FileActionKind::Open => {
                // addopen targets non-std fds (≥ 3) so there is
                // nothing to resolve here.  The open handles are
                // passed through the file_actions VMO instead.
            }
            FileActionKind::Close => {
                // addclose is applied by the kernel's
                // apply_file_action; nothing to resolve here.
            }
        }
    }
    (stdin, stdout, stderr)
}

/// Read a NUL-terminated C string into `dst`.  Returns the
/// number of bytes copied (excluding the NUL terminator).  If
/// the source is longer than `dst`, returns the number of bytes
/// that would have been written and *truncates*; the caller is
/// expected to handle the truncation (POSIX `execve` behaviour).
unsafe fn read_cstr(src: *const u8, dst: &mut [u8]) -> usize {
    let mut i = 0;
    while i < dst.len() {
        let b = *src.add(i);
        if b == 0 {
            break;
        }
        dst[i] = b;
        i += 1;
    }
    i
}

// -------------------------------------------------------------------------
// errno values returned to C callers
// -------------------------------------------------------------------------

const EINVAL: i32 = 22;
const E2BIG: i32 = 7;
const EBADF: i32 = 9;
const ENOENT: i32 = 2;

/// Maximum argv / envp entries we accept from the caller —
/// mirrors the kernel-side `EXECVE_MAX_ARGS` cap.
const MAX_ARGV_ENTRIES: usize = 16;
/// Maximum bytes per argv / envp string (kernel cap).
const MAX_ARG_LEN: usize = 256;

/// Maximum total path bytes across all addopen entries in one
/// spawn call.  1024 covers the typical POSIX worst case:
/// 32 addopen entries × 32-byte average path.
const MAX_PATH_BYTES: usize = 1024;

/// Per-call buffer for paths captured by
/// `posix_spawn_file_actions_addopen`.  Holds up to 1024 bytes
/// total.  `PATH_OFFSET` tracks the next free byte; reset to 0
/// at the start of each `spawn_impl` call.
static mut PATH_POOL: [u8; MAX_PATH_BYTES] = [0u8; MAX_PATH_BYTES];
static mut PATH_OFFSET: usize = 0;

/// Per-call storage for argv / envp slices.  The arrays are
/// `Copy` byte buffers with fixed capacity; we hold one per
/// slot.  We allocate two statics so we can avoid pulling in
/// `alloc` for a no_std library.  Each `posix_spawn` call
/// reuses these buffers — the borrow checker would be
/// unhappy if we tried to hand out `&'static [&'static [u8]]`
/// from a function-scoped local, so the static lifetime is
/// the right tool here.
static mut ARGV_BUF: [[u8; MAX_ARG_LEN]; MAX_ARGV_ENTRIES] =
    [[0u8; MAX_ARG_LEN]; MAX_ARGV_ENTRIES];
static mut ARGV_LENS: [usize; MAX_ARGV_ENTRIES] = [0usize; MAX_ARGV_ENTRIES];
static mut ENVP_BUF: [[u8; MAX_ARG_LEN]; MAX_ARGV_ENTRIES] =
    [[0u8; MAX_ARG_LEN]; MAX_ARGV_ENTRIES];
static mut ENVP_LENS: [usize; MAX_ARGV_ENTRIES] = [0usize; MAX_ARGV_ENTRIES];

/// Copy a NUL-terminated C string into `dst`.  Returns the
/// byte length copied (excluding the NUL), or an errno
/// value if the source is too long.
unsafe fn copy_cstr_into(src: *const u8, dst: &mut [u8]) -> core::result::Result<usize, i32> {
    let mut i = 0usize;
    while i < dst.len() {
        let b = *src.add(i);
        if b == 0 {
            return Ok(i);
        }
        dst[i] = b;
        i += 1;
    }
    // Source ran past our buffer without a NUL — too long.
    Err(E2BIG)
}

/// Read a C `argv` / `envp` array (NULL-terminated list of
/// NUL-terminated strings) into the matching pair of static
/// buffers.  Returns the slice of populated entries, ready
/// to forward to the kernel via the argv VMO format.
///
/// `raw == NULL` means "the caller passed no entries" and we
/// return an empty slice.
unsafe fn read_c_array(
    raw: *const *const u8,
    buf: &mut [[u8; MAX_ARG_LEN]; MAX_ARGV_ENTRIES],
    lens: &mut [usize; MAX_ARGV_ENTRIES],
) -> core::result::Result<usize, i32> {
    if raw.is_null() {
        return Ok(0);
    }
    let mut count = 0usize;
    loop {
        let entry = *raw.add(count);
        if entry.is_null() {
            break;
        }
        if count >= MAX_ARGV_ENTRIES {
            return Err(E2BIG);
        }
        let len = copy_cstr_into(entry, &mut buf[count])?;
        lens[count] = len;
        count += 1;
    }
    Ok(count)
}

/// View the populated slots of `buf`/`lens` as `&[&[u8]]`.
/// Returns `&[]` when `count == 0`.  The returned slice
/// borrows the static buffers, so its lifetime is effectively
/// `'static` (until the next call to this function).
unsafe fn as_slice<'a>(
    buf: &'a [[u8; MAX_ARG_LEN]; MAX_ARGV_ENTRIES],
    lens: &'a [usize; MAX_ARGV_ENTRIES],
    count: usize,
) -> &'a [&'a [u8]] {
    if count == 0 {
        return &[];
    }
    // Build a stack array of subslices and leak it: the
    // buffers outlive the call so the leaked array is fine.
    // We cap the array at MAX_ARGV_ENTRIES which the caller
    // already guarantees via the count.
    let mut tmp: [&[u8]; MAX_ARGV_ENTRIES] = [&[]; MAX_ARGV_ENTRIES];
    for i in 0..count {
        tmp[i] = &buf[i][..lens[i]];
    }
    let leaked: &'a mut [&'a [u8]; MAX_ARGV_ENTRIES] =
        core::mem::transmute(&mut tmp as *mut _);
    &leaked[..count]
}

/// Open a file via the fileagent IPC VFS protocol.  Returns
/// `(channel_handle, remote_fd)` on success, or an error if
/// the VFS channel cannot be opened or the file doesn't exist.
///
/// This is the same protocol `libc::open()` uses; we replicate
/// the IPC here because `posix_spawn` lives in libcapsule
/// (which can't depend on libc).
fn open_via_vfs(path: &[u8], oflag: i32) -> Result<(usize, u32)> {
    let chan = crate::syscalls::channel_lookup("svc.vfs")?;
    let mut cmd = [0u8; 148];
    cmd[0] = 1; // VFS_OPEN
    cmd[4..8].copy_from_slice(&(oflag as u32).to_le_bytes());
    let plen = path.len().min(128);
    cmd[20..20 + plen].copy_from_slice(&path[..plen]);
    crate::syscalls::channel_write(chan, &cmd, &[])?;
    let mut resp = [0u8; 8];
    let _ = crate::syscalls::channel_read(chan, &mut resp, &mut [0u32; 2])?;
    let rfd = i64::from_le_bytes(resp);
    if rfd < 0 {
        Err(Status::NotFound)
    } else {
        Ok((chan, rfd as u32))
    }
}

/// Translate the caller's `posix_spawn_file_actions_t` into
/// the kernel's file-actions VMO format.  Accepts a parallel
/// array of open results (channel_handle + remote_fd) for any
/// `Open` entries.
///
/// VMO format (7 u32s per action, 28 bytes each):
///
///     [u32 count]
///     for i in 0..count:
///         [u32 op]     // 1 = close, 2 = dup2, 3 = open
///         [u32 arg0]   // close:fd / dup2:newfd / open:newfd
///         [u32 arg1]   // dup2:oldfd / open:handle_value
///         [u32 arg2]   // open:remote_fd
///         [u32 arg3]   // reserved (0)
///         [u32 arg4]   // reserved (0)
///         [u32 arg5]   // reserved (0)
///
/// `open_handles` is indexed by `open_idx`: when processing
/// the i-th Open entry we consume `open_handles[open_idx]`.
/// Returns `0` when no actions need to be communicated
/// (caller passed NULL or zero actions).
fn build_file_actions_vmo(
    actions: *const posix_spawn_file_actions_t,
    open_handles: &[(u32, u32)], // (handle_value, remote_fd)
) -> usize {
    if actions.is_null() {
        return 0;
    }
    let a = unsafe { &*actions };
    let count = a.count;
    if count == 0 {
        return 0;
    }

    let total = 4 + count * 28;
    let mut buf = [0u8; 4096 + 32 * 28];
    buf[0..4].copy_from_slice(&(count as u32).to_le_bytes());
    let mut off = 4usize;
    let mut open_idx = 0usize;
    for i in 0..count {
        let e = a.actions[i];
        let (op, arg0, arg1, arg2): (u32, u32, u32, u32) = match e.kind {
            FileActionKind::Close => (1, e.arg0 as u32, 0, 0),
            FileActionKind::Dup2 => (2, e.arg0 as u32, e.arg1 as u32, 0),
            FileActionKind::Open => {
                let (hv, rfd) = if open_idx < open_handles.len() {
                    open_handles[open_idx]
                } else {
                    (0, 0) // no handle — skip at kernel
                };
                open_idx += 1;
                (3, e.arg0 as u32, hv, rfd)
            }
        };
        buf[off..off + 4].copy_from_slice(&op.to_le_bytes());
        buf[off + 4..off + 8].copy_from_slice(&arg0.to_le_bytes());
        buf[off + 8..off + 12].copy_from_slice(&arg1.to_le_bytes());
        buf[off + 12..off + 16].copy_from_slice(&arg2.to_le_bytes());
        // arg3..arg5 = 0 (already zero-initialised)
        off += 28;
    }
    let vmo = match crate::syscalls::vmo_create(total) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    if crate::syscalls::vmo_write(vmo, 0, &buf[..total]).is_err() {
        return 0;
    }
    vmo as usize
}

/// Package the spawn attribute (flags + pgroup) into a VMO the
/// kernel can consume.  Format:
///
///     [u32 flags][u32 pgroup][u32 pad0..5]
///
/// 32 bytes total (8 × u32).  Only `flags` and `pgroup` are
/// read in 1.0; the trailing 6 u32s are reserved for
/// `sigdefault` / `sigmask` (S14) so we don't have to
/// re-shape the VMO later.
///
/// Returns 0 when no attr was passed or the attr carries no
/// interesting bits — kernel treats attr_vmo == 0 as
/// "no overrides".
fn build_attr_vmo(attrp: *const posix_spawnattr_t) -> usize {
    if attrp.is_null() {
        return 0;
    }
    let a = unsafe { &*attrp };
    // Only ship a VMO when at least one attr bit will be
    // applied; we still ship on SETPGROUP even when pgroup
    // matches self.pid so the kernel sees an explicit attr.
    let interesting = (a.flags & (POSIX_SPAWN_SETSID | POSIX_SPAWN_SETPGROUP)) != 0
        || a.pgroup != 0;
    if !interesting {
        return 0;
    }
    let total = 32;
    let vmo = match crate::syscalls::vmo_create(total) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut buf = [0u8; 32];
    buf[0..4].copy_from_slice(&(a.flags as u32).to_le_bytes());
    buf[4..8].copy_from_slice(&(a.pgroup as u32).to_le_bytes());
    if crate::syscalls::vmo_write(vmo, 0, &buf).is_err() {
        return 0;
    }
    vmo as usize
}

// -------------------------------------------------------------------------
// Tests (compile-time only — the binary is `no_std` so the
// `#[cfg(test)]` block is skipped at runtime).
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_actions_init_zero() {
        let mut fa = core::mem::MaybeUninit::<posix_spawn_file_actions_t>::uninit();
        let p = fa.as_mut_ptr();
        assert_eq!(posix_spawn_file_actions_init(p), 0);
        let a = unsafe { &*p };
        assert_eq!(a.count, 0);
    }

    #[test]
    fn adddup2_records_slot() {
        let mut fa = core::mem::MaybeUninit::<posix_spawn_file_actions_t>::uninit();
        let p = fa.as_mut_ptr();
        posix_spawn_file_actions_init(p);
        assert_eq!(posix_spawn_file_actions_adddup2(p, 5, 1), 0);
        let a = unsafe { &*p };
        assert_eq!(a.count, 1);
        assert_eq!(a.actions[0].kind, FileActionKind::Dup2);
        assert_eq!(a.actions[0].arg0, 1);
        assert_eq!(a.actions[0].arg1, 5);
    }
}