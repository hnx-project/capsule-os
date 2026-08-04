use super::{Environment, ShellError};

extern crate libc;
extern crate libcapsule;
extern crate libstd;

/// Handle to the kernel-builtin BootFS VMO.  Every EL0 process shares
/// the same convention (capsule::services::servicesd::BOOTFS_VMO_HANDLE)
/// so the shell can spawn programs from the same image that the
/// service manager reads.
const BOOTFS_VMO_HANDLE: usize = 100;

pub struct CapsuleEnv;

impl CapsuleEnv {
    /// Resolve `cmd` to an absolute path by consulting the `PATH`
    /// environment variable.  On success, the full path is written
    /// into `out` (NUL-terminated).  Returns the same value of
    /// `out` on success.
    ///
    /// Search order:
    ///   1. If `cmd` already contains a `/`, treat it as a path
    ///      and copy it verbatim into `out`.
    ///   2. Walk `PATH` (default `system/bin`) and try each
    ///      concatenation until `open()` succeeds.
    ///   3. Fall back to `system/bin/<cmd>` so the kernel can give
    ///      a more informative error than "file not found".
    fn resolve_command(&self, cmd: &str, out: &mut [u8]) -> Result<(), ShellError> {
        // Case 1: explicit path containing a slash.
        if cmd.contains('/') {
            let len = cmd.len().min(out.len() - 1);
            out[..len].copy_from_slice(cmd.as_bytes());
            out[len] = 0;
            return Ok(());
        }

        // Case 2: walk PATH.  We allow the user to override PATH via
        // the `get_env` callback; if it isn't set we fall back to
        // the CapsuleOS BootFS default `system/bin`.
        let mut path_buf = [0u8; 256];
        let path_len = self.get_env("PATH", &mut path_buf).unwrap_or(0);
        let default_path = b"system/bin";
        let path_str: &[u8] = if path_len > 0 {
            &path_buf[..path_len]
        } else {
            default_path
        };

        // Walk the colon-separated entries.
        let mut start = 0;
        for i in 0..=path_str.len() {
            let at_end = i == path_str.len() || path_str[i] == b':';
            if !at_end {
                continue;
            }
            let dir = &path_str[start..i];
            let need = dir.len() + 1 + cmd.len() + 1;
            if need > out.len() {
                start = i + 1;
                continue;
            }
            let mut j = 0;
            out[j..j + dir.len()].copy_from_slice(dir);
            j += dir.len();
            out[j] = b'/';
            j += 1;
            out[j..j + cmd.len()].copy_from_slice(cmd.as_bytes());
            j += cmd.len();
            out[j] = 0;

            let fd = libc::open(out.as_ptr(), 0, 0);
            if fd >= 0 {
                let _ = libc::close(fd);
                return Ok(());
            }
            start = i + 1;
        }

        // Fall back to the conventional path so the kernel can give
        // a more informative error than "file not found".
        let prefix = b"system/bin/";
        let need = prefix.len() + cmd.len();
        if need < out.len() {
            out[..prefix.len()].copy_from_slice(prefix);
            out[prefix.len()..need].copy_from_slice(cmd.as_bytes());
            out[need] = 0;
        }
        Ok(())
    }
}

impl Environment for CapsuleEnv {
    fn write_stdout(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn write_stderr(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        for i in 0..buf.len() {
            buf[i] = 0;
        }

        let res = libstd::io::read_line(buf);

        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(ShellError::IoError)
        }
    }

    fn getcwd(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        // `libstd::env::current_dir` returns a heap-allocated String
        // that has already been read from the kernel into an internal
        // buffer.  Copy that String into the caller's slice so the
        // builtin can hand it straight to `write_stdout`.  Returning
        // just `s.len()` here would leave `buf` untouched and the
        // builtin would print zero bytes.
        match libstd::env::current_dir() {
            Ok(s) => {
                let bytes = s.as_bytes();
                let len = bytes.len().min(buf.len());
                buf[..len].copy_from_slice(&bytes[..len]);
                Ok(len)
            }
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn chdir(&self, path: &str) -> Result<(), ShellError> {
        match libstd::env::set_current_dir(path) {
            Ok(()) => Ok(()),
            Err(_) => Err(ShellError::PathNotFound),
        }
    }

    fn get_env(&self, _key: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        // 1.0 ships with a hard-coded environment; treat a missing
        // key as "not found" so callers fall back to the default.
        Err(ShellError::PathNotFound)
    }

    fn set_env(&self, _key: &str, _value: &str) -> Result<(), ShellError> {
        Ok(())
    }

    fn print_envs(&self) {}

    fn execute(&self, _cmd: &str, _args: &[&str]) -> Result<i32, ShellError> {
        // CapsuleOS does not implement execve() at the syscall level
        // yet; users that need to replace the shell process should
        // use `spawn` followed by `_exit(0)`.
        Err(ShellError::IoError)
    }

    fn spawn(&self, cmd: &str, args: &[&str]) -> Result<u64, ShellError> {
        // 1. PATH / explicit-path resolution.  We don't actually use
        //    the resolved path for the lookup — CapsuleOS programs
        //    live in BootFS as flat entries keyed by basename
        //    (e.g. `ls`, `cat`, `mkdir`, ...), so we strip the
        //    directory components from `cmd` and hand the basename
        //    to `ProgramLoader::spawn_program`.
        let mut resolved = [0u8; 384];
        self.resolve_command(cmd, &mut resolved)?;

        // 2. Strip down to the basename for the BootFS lookup.
        let bytes = cmd.as_bytes();
        let mut start = 0;
        for i in (0..bytes.len()).rev() {
            if bytes[i] == b'/' {
                start = i + 1;
                break;
            }
        }
        let basename = &bytes[start..];
        let basename_str = core::str::from_utf8(basename)
            .map_err(|_| ShellError::PathNotFound)?;

        let loader = libcapsule::ProgramLoader::new(BOOTFS_VMO_HANDLE);

        // Build the argv block the kernel's `sys_spawn_std` expects.
        // 16 slots matches the kernel's `EXECVE_MAX_ARGS = 16`.
        // Each slot holds a tiny inline buffer (32 bytes is enough
        // for `ls /system` and the like); if the user passes a
        // longer argument we refuse to forward it (rare in 1.0).
        const ARG_LEN: usize = 32;
        let mut argv_buf = [[0u8; ARG_LEN]; 16];
        let mut argv_lens = [0usize; 16];
        let mut argv_storage: [&[u8]; 16] = [&[]; 16];
        let mut argc = 0usize;

        // argv[0] = program name (always the basename).
        let n0 = basename_str.len().min(argv_buf[argc].len());
        argv_buf[argc][..n0].copy_from_slice(basename_str.as_bytes());
        argv_lens[argc] = n0;
        argc += 1;

        // argv[1..] = user-supplied args.
        for &a in args.iter().take(15) {
            let n = a.len().min(argv_buf[argc].len());
            if n != a.len() {
                break;
            }
            argv_buf[argc][..n].copy_from_slice(a.as_bytes());
            argv_lens[argc] = n;
            argc += 1;
        }

        for i in 0..argc {
            argv_storage[i] = &argv_buf[i][..argv_lens[i]];
        }

        let pid = match loader.spawn_program(basename_str) {
            Ok(p) => p as u64,
            Err(_) => return Err(ShellError::PathNotFound),
        };

        let _ = args;
        Ok(pid)
    }

    fn pipe(&self, fds: &mut [i32; 2]) -> Result<(), ShellError> {
        let mut raw = [0i32; 2];
        match libc::syscalls::pipe_pair(&mut raw) {
            Ok(()) => {
                fds[0] = raw[0];
                fds[1] = raw[1];
                Ok(())
            }
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn dup2(&self, old_fd: i32, new_fd: i32) -> Result<(), ShellError> {
        match libc::syscalls::dup2(old_fd, new_fd) {
            Ok(_) => Ok(()),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn wait(&self, pid: u64) -> Result<i32, ShellError> {
        // POSIX wait4() semantics: block until the named child exits,
        // then reap its exit status.  CapsuleOS's
        // `sys_wait4` returns `TryAgain` while the child is still
        // alive, so we issue a syscall-yield (`SYSCALL_YIELD`) on
        // every retry, which lets the scheduler give the child
        // process a chance to run.  A pure `libstd::thread::yield_now`
        // would only nudge the same-hart thread; we need the
        // cross-hart ready-queue scan that the kernel timer's
        // scheduler invocation provides.
        let mut status: i32 = 0;
        loop {
            match libc::syscalls::wait4(pid as i64, &mut status, 0) {
                Ok((_, _)) => return Ok(status),
                Err(libcapsule::Status::TryAgain) => {
                    let _ = libcapsule::syscalls::yield_cpu();
                }
                Err(_) => return Err(ShellError::IoError),
            }
        }
    }

    fn yield_cpu(&self) {
        libstd::thread::yield_now();
    }

    fn open(&self, path: &str, _flags: i32) -> Result<i32, ShellError> {
        match libstd::fs::File::open(path) {
            Ok(file) => {
                let fd = file.fd;
                core::mem::forget(file);
                Ok(fd)
            }
            _ => Err(ShellError::PathNotFound),
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, ShellError> {
        let file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(fd) };
        let res = file.read(buf);
        core::mem::forget(file);
        match res {
            Ok(n) => Ok(n),
            _ => Err(ShellError::IoError),
        }
    }

    fn write(&self, fd: i32, data: &[u8]) -> Result<usize, ShellError> {
        let file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(fd) };
        let res = file.write(data);
        core::mem::forget(file);
        match res {
            Ok(n) => Ok(n),
            _ => Err(ShellError::IoError),
        }
    }

    fn read_file(&self, _path: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        Err(ShellError::IoError)
    }

    fn exit(&self, _code: i32) -> ! {
        panic!("Shell exited");
    }
}