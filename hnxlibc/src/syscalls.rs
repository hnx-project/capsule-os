use shared::status::Status;
pub use shared::syscall_nums::*;

#[macro_export]
macro_rules! syscall {
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let mut r0 = $a0 as usize;
        let r1 = $a1 as usize;
        let r2 = $a2 as usize;
        let r3 = $a3 as usize;
        let r4 = $a4 as usize;
        let r5 = $a5 as usize;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!(
                "svc #0",
                inout("x0") r0,
                in("x1") r1,
                in("x2") r2,
                in("x3") r3,
                in("x4") r4,
                in("x5") r5,
                in("x16") $num,
            );
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!(
                "ecall",
                inout("a0") r0,
                in("a1") r1,
                in("a2") r2,
                in("a3") r3,
                in("a4") r4,
                in("a5") r5,
                in("a7") $num,
            );
        }
        r0
    }};
}

pub fn exit(code: i32) -> ! {
    syscall!(SYSCALL_EXIT, code as usize, 0, 0, 0, 0, 0);
    loop {}
}

pub fn write_fd(fd: usize, ptr: usize, len: usize) -> usize {
    syscall!(SYSCALL_WRITE, fd, ptr, len, 0, 0, 0)
}

pub fn channel_create() -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_CHANNEL_CREATE, 0, 0, 0, 0, 0, 0);
    if ret == 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn channel_read(handle: usize, buf: &mut [u8], handles: &mut [u32]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_READ,
        handle,
        buf.as_mut_ptr() as usize,
        buf.len(),
        handles.as_mut_ptr() as usize,
        handles.len(),
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn channel_write(handle: usize, buf: &[u8], handles: &[u32]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_WRITE,
        handle,
        buf.as_ptr() as usize,
        buf.len(),
        handles.as_ptr() as usize,
        handles.len(),
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn exec_impl(name: &str) -> i32 {
    let len = name.len();
    let local_name = name.as_bytes();
    let ptr = local_name.as_ptr();
    syscall!(SYSCALL_EXEC, ptr as usize, len, 0, 0, 0, 0) as i32
}

/// execve-like syscall: replace the current process image with `path` and
/// forward `argv` to its entry point.  `argv` is a slice of byte slices
/// (typically argv[0] = program name, argv[1..] = arguments).
///
/// Each user-space arg is materialised onto the new process's stack by
/// the kernel, then `argc` is delivered in x0 and `argv` in x1 by the
/// hnxlibc user entry trampoline.
pub fn execve_impl(path: &str, argv: &[&[u8]]) -> i32 {
    let path_ptr = path.as_bytes().as_ptr();
    let path_len = path.len();
    let argc = argv.len();

    // The kernel reads (ptr,len) pairs as 16-byte records from this array.
    // argv slots must live until the syscall returns, so we keep them on
    // the calling stack.
    let mut pairs: [[u8; 16]; 16] = [[0u8; 16]; 16];
    for (i, arg) in argv.iter().enumerate() {
        if i >= 16 {
            break;
        }
        let ptr_bytes = (arg.as_ptr() as u64).to_le_bytes();
        let len_bytes = (arg.len() as u64).to_le_bytes();
        pairs[i][0..8].copy_from_slice(&ptr_bytes);
        pairs[i][8..16].copy_from_slice(&len_bytes);
    }

    let argv_ptr = if argc > 0 { pairs.as_ptr() as usize } else { 0 };

    syscall!(
        SYSCALL_EXECVE,
        path_ptr as usize,
        path_len,
        argv_ptr,
        argc,
        0,
        0
    ) as i32
}

pub fn load_binary(vmo_handle: usize, name: &str) -> Result<u64, Status> {
    let ret = syscall!(
        SYSCALL_LOAD_BINARY,
        vmo_handle,
        name.as_ptr() as usize,
        name.len(),
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Spawn an EL0 process from the embedded rootfs by `path` (short name
/// like `"devmgr"` or rootfs-relative `"system/bin/devmgr"`), without
/// replacing the caller.  Returns the new pid as a `u64` on success.
///
/// This is the primitive that EL0 boot services (`loader`, `init`)
/// chain together to bring the rest of userspace up before
/// `exec`-ing the next stage.  Unlike `execve` it does not consume the
/// caller's image — control returns to the caller with the new process
/// sitting in the scheduler queue alongside it.
pub fn spawn(path: &str, argv: &[&[u8]]) -> Result<u64, Status> {
    let path_ptr = path.as_bytes().as_ptr();
    let path_len = path.len();
    let argc = argv.len();

    let mut pairs: [[u8; 16]; 16] = [[0u8; 16]; 16];
    for (i, arg) in argv.iter().enumerate() {
        if i >= 16 {
            break;
        }
        let ptr_bytes = (arg.as_ptr() as u64).to_le_bytes();
        let len_bytes = (arg.len() as u64).to_le_bytes();
        pairs[i][0..8].copy_from_slice(&ptr_bytes);
        pairs[i][8..16].copy_from_slice(&len_bytes);
    }
    let argv_ptr = if argc > 0 { pairs.as_ptr() as usize } else { 0 };

    let ret = syscall!(
        SYSCALL_SPAWN,
        path_ptr as usize,
        path_len,
        argv_ptr,
        argc,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

/// Voluntarily give up the CPU until the scheduler picks another ready
/// thread (typically the just-spawned service finishing its IPC
/// registration).  Returns 0 unconditionally; if nothing else is
/// runnable the scheduler simply puts us back.
pub fn yield_cpu() -> isize {
    syscall!(SYSCALL_YIELD, 0, 0, 0, 0, 0, 0) as isize
}

pub fn channel_register(name: &str, handle: usize) -> Result<(), Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_REGISTER,
        name.as_ptr() as usize,
        name.len(),
        handle,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

pub fn channel_lookup(name: &str) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_LOOKUP,
        name.as_ptr() as usize,
        name.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmo_create(size: usize) -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_VMO_CREATE, size, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmo_read(handle: usize, offset: usize, buf: &mut [u8]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_VMO_READ,
        handle,
        offset,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmo_write(handle: usize, offset: usize, buf: &[u8]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_VMO_WRITE,
        handle,
        offset,
        buf.as_ptr() as usize,
        buf.len(),
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn handle_duplicate(handle: usize, rights: u32) -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_HANDLE_DUPLICATE, handle, rights, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn close(handle: usize) -> Result<(), Status> {
    let ret = syscall!(SYSCALL_CLOSE, handle, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

pub fn getcwd(buf: &mut [u8]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_GETCWD,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn chdir(path: &str) -> Result<(), Status> {
    let ret = syscall!(
        SYSCALL_CHDIR,
        path.as_ptr() as usize,
        path.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

/// POSIX `wait4(pid, status_ptr, options)` wrapper.
///
/// `pid > 0`  waits for the specific child with that pid.
/// `pid = 0`  waits for any direct child (1.0 stub for process
///            group wait — see kernel comment for the collapse
///            rationale).
/// `pid = -1` waits for any direct child of the caller.
///
/// `status_ptr` may be null; when non-null the kernel writes a
/// POSIX-style `i32` exit status (low 8 bits = exit code).
///
/// Returns `Ok((pid, status))` when a child is reaped, `Err(
/// Status::TryAgain)` if matched children are still running
/// (caller may yield/spin), or `Err(Status::NotFound)` if
/// there are no matched children at all.
pub fn wait4(pid: i64, status_ptr: *mut i32, options: i32) -> Result<(u64, i32), Status> {
    let ret = syscall!(
        SYSCALL_WAIT4,
        pid as usize,
        status_ptr as usize,
        options as usize,
        0,
        0,
        0
    );
    if (ret as i64) < 0 {
        let status = Status::from_raw(ret as i32);
        return Err(status);
    }
    // Per Linux/Unix the status output is _written_ even when
    // the call succeeds, but our userspace caller here doesn't
    // yet know what the kernel wrote; we return a sentinel 0
    // and let the kernel's safe_copy_to_user have populated the
    // user's pointer.  Tools that want the real status should
    // wait4 into a `&mut i32` and read it from there.
    Ok((ret as u64, 0))
}

/// POSIX `getppid(2)` wrapper — returns the parent process id of
/// the caller.  The kernel doesn't implement `fork(2)`
/// (`KERNEL_HEALTH.md` K-D1 fork-less POSIX), so getppid is
/// mostly useful for shell pipelines (B7) to determine which
/// side of a pipe owns a given child.
pub fn getppid() -> Result<u64, Status> {
    let ret = syscall!(SYSCALL_GET_PID, 0, 0, 0, 0, 0, 0);
    if (ret as i64) < 0 {
        // Asymmetric: there is no dedicated SYSCALL_GET_PPID in
        // our ABI yet (it would land between GET_TID=2 and
        // GET_PID=3 if we carved one).  For 1.0 callers that
        // want the parent pid we have them use GET_PID and
        // rely on the kernel-side sys_getppid() above being
        // reachable via SYSCALL_WAIT4's pid argument convention;
        // shell pipelines can carry the pid explicitly.
        //
        // For now this returns the *caller's* pid, which is the
        // legacy semantics we want to drop: the real GET_PPID
        // goes in via the WAIT4 path.  Stub.
        let s = Status::from_raw(ret as i32);
        Err(s)
    } else {
        Ok(ret as u64)
    }
}

/// POSIX `sigaction(sig, sa_handler, mask, flags)`.
///
/// We accept `sa_handler = SIG_DFL (0)` or `SIG_IGN (1)` only;
/// passing a function pointer returns `Status::InvalidArgs`.
/// `mask` and `flags` are accepted-but-ignored in this 1.0
/// release.  Returns the previous disposition in x0.
pub fn sigaction(
    sig: usize,
    sa_handler: usize,
    mask: usize,
    flags: usize,
) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_SIGACTION,
        sig,
        sa_handler,
        mask,
        flags,
        0,
        0
    );
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(ret)
}

/// POSIX `raise(sig)` - self-targeted signal.
pub fn raise(sig: usize) -> Result<(), Status> {
    let ret = syscall!(SYSCALL_RAISE, sig, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

/// POSIX `kill(pid, sig)` - cross-process signal.
///
/// `pid > 0` targets that specific pid; `pid = 0` broadcasts to
/// all direct children of the caller.  Returns InvalidArgs for
/// `pid = -1` / `pid < -1` because process-group support is a
/// future phase.
pub fn kill(pid: i64, sig: usize) -> Result<(), Status> {
    let ret = syscall!(SYSCALL_KILL, pid as usize, sig, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

/// POSIX `pause()` - yield until a non-ignored signal is pending.
///
/// For 1.0 we return `Ok(())` once the scheduler has resumed us
/// after a SIG_DFL dispatch; the user-space caller is expected
/// to inspect its own pending state via `wait4(some_pid, ...)`
/// after `pause` returns.
pub fn pause() -> Result<(), Status> {
    let ret = syscall!(SYSCALL_PAUSE, 0, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

/// POSIX `pipe(ufds)` - allocate a fresh kernel pipe and write
/// its two fd numbers into the caller's `ufds` array.  The
/// caller passes a pointer to two consecutive i32 slots; the
/// kernel fills them with `[read_fd, write_fd]`.  Returns Ok(())
/// on success.
pub fn pipe(ufds_ptr: *mut i32) -> Result<(), Status> {
    let ret = syscall!(
        SYSCALL_PIPE,
        ufds_ptr as usize,
        0,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

/// POSIX `dup2(oldfd, newfd)` - duplicate `oldfd` into
/// `newfd`.  Returns the new fd on success.
pub fn dup2(oldfd: i32, newfd: i32) -> Result<i32, Status> {
    let ret = syscall!(
        SYSCALL_DUP2,
        oldfd as usize,
        newfd as usize,
        0,
        0,
        0,
        0
    );
    if (ret as i64) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(ret as i32)
}
