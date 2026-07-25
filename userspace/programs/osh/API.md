# 📁 osh — CapsuleOS Shell

## Status
[Active (使用中)]

## Name
**osh** — minimal POSIX-compatible shell for CapsuleOS.

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `libstd` | Print/output, argv iteration |
| `libc` | Raw POSIX syscalls (`read`, `write`, `open`, `close`, `execv`, `pipe`, `dup2`, `wait4`) |
| `libcapsule` | VMO creation, handle-based `spawn` via VMO+argv |
| `fileagent` | File I/O for script execution |
| `devmgr` | Device node access (`/dev/tty`) |

## Core Definition
`osh` is CapsuleOS's interactive and scriptable command shell. It parses a pipeline grammar (`|` separators, single-command builtins), manages process spawning via kernel syscalls, and provides readline-style history navigation (S8).

The shell runs as an EL0 user-space program. All process lifecycle, I/O, and IPC flow through the microkernel's capability-based syscall interface and the `fileagent` service — never through direct kernel data structures.

## Exposed Interfaces

### Pipeline Execution (`shell.rs`)
- `execute_pipeline(env, line)` — parse and run a single pipeline (one or more `|`-separated commands).
- `run_shell(env)` — interactive REPL. On `#[cfg(not(feature = "host"))]` uses the `Readline` module for history; on host mode uses the `Environment::read_line` trait method.
- `run_script(env, path)` — read and execute a script file line by line.

### Readline (`readline.rs`)
- `Readline::new()` — create a fresh readline state with an empty history ring (16 slots, 256 bytes each).
- `Readline::read_line(env) -> Result<usize, ()>` — read one edited line from stdin. Handles:
  - Regular text entry (appended to buffer)
  - Backspace (`0x7f` / `0x08`) — kernel-cooked erase
  - Ctrl+C (`0x03`) — abort line
  - Ctrl+D (`0x04`) — EOF on empty line
  - Up/Down arrow — history ring navigation (`\x1b[A` / `\x1b[B`)
- `Readline::line() -> &[u8]` — the current line buffer after a successful read.

### Builtins (`builtins/`)
- `cd <path>` — change directory (delegates to `env.chdir`).
- `pwd` — print working directory (delegates to `env.getcwd`).
- `exit [code]` — terminate the shell.
- `export KEY=VALUE` — set environment variable.
- `env` — print all environment variables.

### Environment Trait (`env/mod.rs`)
The `Environment` trait abstracts all OS interactions so `osh` can run on both CapsuleOS (`CapsuleEnv`) and the host OS (`HostEnv` for testing).

| Method | Description |
|--------|-------------|
| `write_stdout(data)` | Write bytes to standard output |
| `write_stderr(data)` | Write bytes to standard error |
| `read_line(buf)` | Read a line from standard input (host mode) |
| `read(fd, buf) -> usize` | Raw read from a file descriptor |
| `write(fd, data) -> usize` | Raw write to a file descriptor |
| `getcwd(buf) -> usize` | Get current working directory path |
| `chdir(path)` | Change working directory |
| `get_env(key, buf) -> usize` | Get environment variable |
| `set_env(key, value)` | Set environment variable |
| `print_envs()` | Print all environment variables |
| `spawn(cmd, args) -> pid` | Spawn a new process |
| `pipe(fds) -> [i32; 2]` | Create a pipe |
| `dup2(old, new)` | Duplicate a file descriptor |
| `wait(pid) -> exit_code` | Wait for a child process |
| `yield_cpu()` | Yield the CPU |
| `open(path, flags) -> fd` | Open a file |
| `read_file(path, buf) -> usize` | Read entire file into buffer |
| `exit(code)` | Terminate with exit code |

### Parser (`parser.rs`)
- `parse_line(line) -> Option<Pipeline>` — tokenise and parse a single command line into a `Pipeline` structure.
- Pipeline stages support: command name, positional args, `|` separators, `#` line comments.

## History
| Version | Change |
|---------|--------|
| 1.0.48  | S8: readline module with history (up/down arrows), Ctrl+C/D, `\x1b[K` redraw |
| 1.0.47  | S7: procmgr std-fd handoff init |
| 1.0.46  | S2: real pipe/dup2 in pipeline runner |
