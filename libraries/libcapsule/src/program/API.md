# 📁 `libcapsule::program` — Sandbox Loader for BootFS Programs

## Status
[Active (使用中)]

## Name
**`ProgramLoader`** — spawn ordinary sandboxed EL0 applications (e.g. `testall`, `cat`, `ls`) from the read-only boot-time archive (`BootFS`).

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `libcapsule::service::ServiceLoader` | Underlying loader that resolves BootFS entries and exposes the raw bootfs_vmo handle. |
| `libcapsule::syscalls::{vmo_create_child, vmo_create, vmo_write, close, load_binary}` | Construct the program binary VMO child, the argv VMO, and (for `spawn_program_with_std_fds`) the std-fds VMO; `load_binary` runs the non-std path. |
| `kernel::syscall::handlers::process::sys_spawn_std` | The kernel-side handler `spawn_program_with_std_fds` ultimately dispatches to. |
| `shared::syscall_nums::SYSCALL_SPAWN_STD` | The syscall number dispatched by `spawn_program_with_std_fds`. |
| `kernel::arch::aarch64::vfs` | Walks the `HNXF_VFS` superblock during `resolve_file`. |
| `libcapsule::posix_spawn` | Higher-level wrapper that exposes the POSIX `posix_spawn(3)` API on top of `ProgramLoader::spawn_program_with_std_fds`. |

## Core Definition
CapsuleOS spawning is **not** based on the classic UNIX `fork()` — a fresh capability container is built for each program from the read-only BootFS image. `ProgramLoader` exposes two entry points:

* `spawn_program(name)` — the historic B-series loader: clone the program into a fresh VMO, then call `load_binary`. The new process starts with the kernel-builtin UART fds 0/1/2.
* `spawn_program_with_std_fds(name, stdin, stdout, stderr, argv, envp, file_actions_vmo)` — the S7+ procmgr std-fd handoff variant. Same VMO setup, but the spawner also hands the kernel three channel handles that are translated into the child's `fd_table[0..=2]`. A `0` entry leaves the matching slot on the kernel-builtin UART path. The dispatch avoids `load_binary` (which exec-replaces the caller) so procmgr keeps running while its children spin up. `argv` and `envp` are forwarded as VMOs in the same format the kernel's `sys_spawn` argv parser uses (`[u32 argc][u32 strlen][bytes]…`); both are bounded at 16 entries / 256 bytes each to match the kernel's `EXECVE_MAX_ARGS`. `file_actions_vmo` carries the `posix_spawn_file_actions_t` table built by `libcapsule::posix_spawn::build_file_actions_vmo`; pass `0` to skip.

> **Note**: prefer `libcapsule::posix_spawn` for new code.  The POSIX
> `posix_spawn(3)` interface sits on top of these primitives and gives
> callers the standard file_actions / attribute object ergonomics.

## Exposed Interfaces

### Constructor
| Function | Purpose |
|----------|---------|
| `pub const fn new(bootfs_vmo: usize) -> Self` | Bind a loader to a raw `HandleValue` for the boot memory filesystem. |

### Spawn variants
| Function | Purpose |
|----------|---------|
| `pub fn spawn_program(&self, name: &str) -> Result<usize>` | Spawn `name` from BootFS into a fresh process. Returns the new pid. Errors: `NotFound` / `WrongType` / `InvalidImage` / `NoMemory`. |
| `pub fn spawn_program_with_std_fds(&self, name: &str, stdin: u32, stdout: u32, stderr: u32, argv: &[&[u8]], envp: &[&[u8]], file_actions_vmo: usize) -> Result<usize>` | Spawn `name` and install the three supplied channel handles into the child's `fd_table[0..=2]`. Forward the caller's argv / envp entries, plus any `posix_spawn_file_actions_t` VMO built by `libcapsule::posix_spawn::build_file_actions_vmo`. A `0` slot falls back to the kernel-builtin UART. |

### Args / std-fds VMO layout (kernel-visible)
* **argv VMO**: `[u32 argc][u32 strlen][bytes]` for each argv entry.  Capped at 16 entries / 256 bytes each to match the kernel's `EXECVE_MAX_ARGS`.  argv[0] is always the program name.
* **envp VMO**: same encoding as argv but holds the environment strings.  The kernel materialises envp onto the child user stack just below argv, with x2=envc and x3=envp_ptr passed through the trampoline so `getenv(3)` sees the forwarded values once the child boots.
* **std-fds VMO**: `[u32 stdin][u32 stdout][u32 stderr]` — three little-endian channel handles packed in 12 bytes.
* **file-actions VMO**: `[u32 count][u32 op][u32 arg0][u32 arg1]…` per entry; op is `1` = close, `2` = dup2.  See `libcapsule::posix_spawn/API.md` for the full state of the 1.0 subset.

## Notes / Future Work

* argv / envp forwarders accept at most 16 entries × 256 bytes each, matching the kernel.  Pass `&[]` when no extra argv / envp slots are needed.
* The wrapper does **not** clone the parent's handle table into the child. The child starts with the default empty handle table; any handles it needs must be re-acquired after spawn via the loader's startup handshake.
* envp is materialised onto the child user stack by the kernel and exposed through `__HNX_ENVC` / `__HNX_ENVP_PTRS` / `__HNX_ENVP_LENS` (read by the libc trampoline; see `hnx_envp()`).  Default `PATH`/`HOME`/`USER`/etc. are filled in by `env_init()` if the spawner didn't pass them.