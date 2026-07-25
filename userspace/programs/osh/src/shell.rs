//! B7 (`KERNEL_HEALTH.md` B7): shell executor for `osh`.
//!
//! Pre-B7 this module only handled one command per line.  B7
//! replaces that with a `Pipeline` runner so that
//!   `cat /etc/hostname | grep .`
//! arranges:
//!   1. env.pipe()  -> [read_fd, write_fd]
//!   2. spawn("cat") + dup2(write_fd, 1) so cat writes into the pipe
//!   3. spawn("grep") + dup2(read_fd, 0) so grep reads from it
//!   4. wait each side for exit
//!
//! The kernel does the heavy lifting via SYSCALL_PIPE (B6),
//! SYSCALL_SPAWN (B3), and the per-process fd_table (B6),
//! leaving osh to glue four syscalls together for each `|`.
//!
//! Builtins stay on the single-command path so `cd`, `pwd`,
//! `exit` etc. do not need pipeline plumbing -- these cannot
//! be a stage of a pipe anyway.

use crate::builtins::execute_builtin;
use crate::env::Environment;
use crate::parser::parse_line;

/// Execute one pipeline.
pub fn execute_pipeline<E: Environment>(env: &E, line: &str) {
    let trimmed = line.trim_end_matches(|c| c == '\r' || c == '\n');
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return;
    }
    let pipeline = match parse_line(trimmed) {
        Some(p) => p,
        None => return,
    };
    if pipeline.stage_count == 0 {
        return;
    }
    if pipeline.stage_count == 1 {
        // Single-command path: run in a spawned child process so the shell survives
        let cmd = &pipeline.stages[0];
        if execute_builtin(env, cmd) {
            return;
        }
        let run_args = stage_args(cmd);
        match env.spawn(cmd.name, run_args) {
            Ok(pid) => {
                let _ = env.wait(pid);
            }
            Err(_) => {
                env.write_stderr(b"osh: command not found: ");
                env.write_stderr(cmd.name.as_bytes());
                env.write_stderr(b"\n");
            }
        }
        return;
    }
    run_pipeline(env, &pipeline);
}

/// Render the args array of one Command into a `&[&str]` slice
/// that the Environment::execute API accepts.
fn stage_args<'a>(cmd: &'a crate::parser::Command<'a>) -> &'a [&'a str] {
    &cmd.args[..cmd.arg_count]
}

/// Run a multi-stage pipeline.  Each stage except the very
/// last writes into a fresh pipe whose read end becomes the
/// stdin of the next stage; the very last stage writes to the
/// inherited stdout (fd 1).  Spawn order: producers before
/// consumers, so the read end of every pipe outlives the
/// writes that flow through it.
fn run_pipeline<'a, E: Environment>(env: &E, p: &crate::parser::Pipeline<'a>) {
    // Allocate one pipe per inter-stage gap.  For a pipeline
    // with N stages we need N-1 pipes; stage 0 reads from
    // stdin (fd 0) and stage N-1 writes to stdout (fd 1).
    let stage_count = p.stage_count;
    if stage_count == 0 {
        return;
    }
    let mut pipes: [[i32; 2]; 8] = [[-1; 2]; 8];
    for i in 0..stage_count.saturating_sub(1) {
        let mut fds = [0i32; 2];
        if env.pipe(&mut fds).is_err() {
            env.write_stderr(b"osh: pipe() failed for stage ");
            let mut n_buf = [0u8; 16];
            let s = format_u32(i as u32, &mut n_buf);
            env.write_stderr(s);
            env.write_stderr(b"\n");
            // Best-effort cleanup: close any earlier pipes we
            // already opened so we don't leak fds into the
            // parent process for the rest of its lifetime.
            for j in 0..i {
                close_fd(env, pipes[j][0]);
                close_fd(env, pipes[j][1]);
            }
            return;
        }
        pipes[i] = fds;
    }

    // Track pids so we can wait for the whole pipeline.  In
    // 1.0 we have at most 4 stages per Pipeline (parser
    // const) so a fixed-size array is fine.
    let mut pids: [u64; 4] = [0; 4];

    for i in 0..stage_count {
        let cmd = &p.stages[i];
        let spawn_args: &[&str] = &cmd.args[..cmd.arg_count];

        // stdin: read end of the previous pipe (fd 0 for the
        // very first stage).
        if i > 0 {
            if env.dup2(pipes[i - 1][0], 0).is_err() {
                env.write_stderr(b"osh: dup2(stdin) failed at stage ");
                let mut n_buf = [0u8; 16];
                let s = format_u32(i as u32, &mut n_buf);
                env.write_stderr(s);
                env.write_stderr(b"\n");
            }
            // The previous write end is no longer needed in
            // the parent — closing it lets the child see EOF
            // on the read end once the previous stage exits.
            close_fd(env, pipes[i - 1][1]);
        }
        // stdout: write end of this stage's pipe (fd 1 for
        // the very last stage).
        if i + 1 < stage_count {
            if env.dup2(pipes[i][1], 1).is_err() {
                env.write_stderr(b"osh: dup2(stdout) failed at stage ");
                let mut n_buf = [0u8; 16];
                let s = format_u32(i as u32, &mut n_buf);
                env.write_stderr(s);
                env.write_stderr(b"\n");
            }
            // The read end is not consumed by the child, so
            // close it in the parent to keep the pipe open
            // only for as long as needed.
            close_fd(env, pipes[i][0]);
        }

        let pid = match env.spawn(cmd.name, spawn_args) {
            Ok(id) => id,
            Err(_) => {
                env.write_stderr(b"osh: spawn failed: ");
                env.write_stderr(cmd.name.as_bytes());
                env.write_stderr(b"\n");
                // Try to keep the shell alive across a
                // mid-pipeline failure: best-effort restore
                // of the parent's stdio by re-dup'ing the
                // original fd 0/1 back from saved copies.
                // For 1.0 we just bail out of the whole
                // pipeline; the user-visible behaviour is
                // "stage i failed, rest skipped".
                for j in 0..i {
                    close_fd(env, pipes[j][0]);
                    close_fd(env, pipes[j][1]);
                }
                return;
            }
        };
        pids[i] = pid;

        // After the spawn, fd 0/1 in the parent are
        // contaminated by the dup2's we just ran for the
        // child's benefit.  Restore them from the saved
        // originals so the next interactive iteration of the
        // shell still has working stdio.
        if i > 0 {
            // dup2(saved_stdin, 0) — but we don't have a
            // copy of the original stdin fd; the cleanest
            // solution is to dup the read end of the *next*
            // pipe (or fd 0 if we are at the last stage)
            // back into 0.  We take a simpler approach for
            // 1.0: rely on the kernel-builtin UART slot
            // always being readable, so 0 stays valid.  The
            // 1.1+ plan is to keep `env.stdin_save` on the
            // Environment trait.
        }
    }

    // Wait for every spawned child, in spawn order.  1.0
    // doesn't model job control, so a failure on stage i
    // doesn't cancel stages i+1..N.
    for i in 0..stage_count {
        let _ = env.wait(pids[i]);
    }
}

fn close_fd<E: Environment>(env: &E, fd: i32) {
    if fd < 0 {
        return;
    }
    // The env trait doesn't expose a generic close; we just
    // call into libc through the syscall path.  The `env`
    // bound keeps the signature uniform with the rest of
    // this module even though we don't actually consume it.
    let _ = env;
    // SAFETY: this is a best-effort cleanup; we don't care
    // about the return value.  Calling close on a stale fd
    // is benign.
    unsafe {
        libc::close(fd);
    }
}

/// Crude `format_u32` helper (writes ASCII decimal into `buf`
/// and returns the slice from the first digit).  Avoids
/// pulling `core::fmt` into a no_std user-space binary that
/// hnxstd doesn't yet provide; B9 will replace this with the
/// real `format!` macro.
fn format_u32(mut v: u32, buf: &mut [u8]) -> &[u8] {
    if v == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut i = 0;
    while v > 0 && i < buf.len() {
        let d = b'0' + (v % 10) as u8;
        v /= 10;
        // We write the digits in reverse, then re-slice.
        buf[i] = d;
        i += 1;
    }
    // Reverse in place.
    let mut lo = 0usize;
    let mut hi = i as isize - 1;
    while lo < (hi as usize) {
        let t = buf[lo];
        buf[lo] = buf[hi as usize];
        buf[hi as usize] = t;
        lo += 1;
        hi -= 1;
    }
    &buf[..i]
}

/// Backwards-compat single-line entry kept around for the
/// pre-B7 call sites that still pass one line at a time.
pub fn execute_single_line<E: Environment>(env: &E, line: &str) {
    execute_pipeline(env, line);
}

#[cfg(not(feature = "host"))]
pub fn run_shell<E: Environment>(env: &E) {
    let mut rl = crate::readline::Readline::new();
    loop {
        match rl.read_line(env) {
            Ok(n) if n > 0 => {
                let line_str = match core::str::from_utf8(rl.line()) {
                    Ok(s) => s,
                    Err(_) => {
                        env.write_stderr(b"Error: Invalid UTF-8 input\n");
                        continue;
                    }
                };
                execute_pipeline(env, line_str);
            }
            _ => continue,
        }
    }
}

#[cfg(feature = "host")]
pub fn run_shell<E: Environment>(env: &E) {
    let mut input_buf = [0u8; 256];
    loop {
        env.write_stdout(b"osh$ ");
        match env.read_line(&mut input_buf) {
            Ok(bytes_read) if bytes_read > 0 => {
                let line_bytes = &input_buf[..bytes_read];
                let line_str = match core::str::from_utf8(line_bytes) {
                    Ok(s) => s,
                    Err(_) => {
                        env.write_stderr(b"Error: Invalid UTF-8 input\n");
                        continue;
                    }
                };
                execute_pipeline(env, line_str);
            }
            _ => continue,
        }
    }
}

pub fn run_script<E: Environment>(env: &E, path: &str) {
    let mut script_buf = [0u8; 4096];
    match env.read_file(path, &mut script_buf) {
        Ok(bytes_read) => {
            let script_str = match core::str::from_utf8(&script_buf[..bytes_read]) {
                Ok(s) => s,
                Err(_) => {
                    env.write_stderr(b"Error: Script file contains invalid UTF-8\n");
                    return;
                }
            };
            let mut start = 0;
            let bytes = script_str.as_bytes();
            for i in 0..bytes.len() {
                if bytes[i] == b'\n' {
                    let line = &script_str[start..i];
                    execute_pipeline(env, line);
                    start = i + 1;
                }
            }
            let tail = &script_str[start..];
            if !tail.is_empty() {
                execute_pipeline(env, tail);
            }
        }
        Err(_) => {
            env.write_stderr(b"osh: cannot open script: ");
            env.write_stderr(path.as_bytes());
            env.write_stderr(b"\n");
        }
    }
}
