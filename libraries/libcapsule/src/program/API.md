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
* `spawn_program_with_std_fds(name, stdin, stdout, stderr)` — the S7 / procmgr std-fd handoff variant. Same VMO setup, but the spawner also hands the kernel three channel handles that are translated into the child's `fd_table[0..=2]`. A `0` entry leaves the matching slot on the kernel-builtin UART path. The dispatch avoids `load_binary` (which exec-replaces the caller) so procmgr keeps running while its children spin up.

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
| `pub fn spawn_program_with_std_fds(&self, name: &str, stdin: u32, stdout: u32, stderr: u32) -> Result<usize>` | Spawn `name` and install the three supplied channel handles into the child's `fd_table[0..=2]`. A `0` slot falls back to the kernel-builtin UART. |

### Args / std-fds VMO layout (kernel-visible)
* **argv VMO**: `[u32 argc][u32 strlen][bytes]` for each argv entry.
* **std-fds VMO**: `[u32 stdin][u32 stdout][u32 stderr]` — three little-endian channel handles packed in 12 bytes.

## Notes / Future Work

* `spawn_program_with_std_fds` currently builds an argv of `[name]` only. A future patch will accept the same `&[&str]` shape used by shell pipelines so procmgr can pass the real argv through.
* The wrapper does **not** clone the parent's handle table into the child. The child starts with the default empty handle table; any handles it needs must be re-acquired after spawn via the loader's startup handshake.
* argv length is currently capped at 64 bytes (`argv_buf[4 + 4 + 64]`); longer names fail with `Status::InvalidArgs`.