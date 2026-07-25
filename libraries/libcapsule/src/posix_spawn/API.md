# 📁 `libcapsule::posix_spawn` — Fuchsia-style Process Spawn

## Status
[Active (使用中)]

## Name
**`libcapsule::posix_spawn`** — POSIX `posix_spawn(3)` / `posix_spawnp(3)`
implementation built on top of [`ProgramLoader`](crate::program::ProgramLoader).

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `libcapsule::program::ProgramLoader` | Underlying loader used to resolve BootFS entries and spawn fresh processes.  Both `spawn_program` and `spawn_program_with_std_fds` are exercised here. |
| `libcapsule::syscalls::{channel_create, vmo_create, vmo_write, close}` | Syscall primitives used to build the std-fds VMO and argv VMO. |
| `shared::syscall_nums::SYSCALL_SPAWN_STD` | The kernel syscall number actually dispatched. |
| `libraries::libc::posix_stub` | The libc re-exports every entry point of this module so C callers can `posix_spawn(...)` directly. |
| `libraries::libcapsule/include/capsule_deprecation.h` | Documents the policy of *not* exposing `fork()` at the libc boundary. |

## Core Definition

### Why this exists

CapsuleOS follows the Fuchsia / Zircon process model: **new
processes are created through fresh-container spawn**, not the
legacy `fork+exec` pattern.  The kernel exposes
`SYSCALL_SPAWN_STD` to do this in one step.  This module wraps
that syscall behind the standard POSIX
[`posix_spawn(3)`](https://pubs.opengroup.org/onlinepubs/9699919799/functions/posix_spawn.html)
API so userland programs can launch children without any need
to call `fork()` directly.

### What is implemented (Pangu 1.0 subset)

* `posix_spawn` / `posix_spawnp` — main entry points.
* `posix_spawn_file_actions_init/destroy` — lifecycle.
* `posix_spawn_file_actions_addopen` — recorded; the actual
  open is *not yet* performed by the kernel-side loader.
  Recorded entries do take up a slot, so a future kernel-side
  open pass will see the action list intact.
* `posix_spawn_file_actions_addclose` — **fully wired**:
  closed in the freshly-spawned child's fd_table by
  `apply_file_action` in `sys_spawn_std`.
* `posix_spawn_file_actions_adddup2` — **fully wired**: when
  the target slot is `0`/`1`/`2` the dup2 is folded into the
  std-fds VMO and the child boots with the redirected std fd.
* `posix_spawnattr_init/destroy` and the
  `_set{flags,pgroup,sigdefault,sigmask}` helpers.
* `POSIX_SPAWN_RESETIDS`, `POSIX_SPAWN_SETPGROUP`,
  `POSIX_SPAWN_SETSIGDEF`, `POSIX_SPAWN_SETSIGMASK`,
  `POSIX_SPAWN_SETSID`, plus the CapsuleOS extension
  `POSIX_SPAWN_WAITPID`.

### argv / envp forwarding

The 1.0 implementation walks the caller's C `argv` / `envp`
arrays, copies each entry into a per-call static buffer
(max 16 entries, max 256 bytes per entry), and packages
them into the same `[u32 argc][u32 strlen][bytes]…` VMO
layout the kernel's `sys_spawn` already parses.  Both arrays
are forwarded to `SYSCALL_SPAWN_STD` as separate VMOs.

In 1.0 the kernel-side `launch_user_program_with_argv_and_envp`
materialises both argv AND envp onto the child user stack:
argv strings are 16-byte aligned, NUL-terminated, with the
pointer array at the lowest address; envp is materialised
just below argv with the same encoding.  The user entry
trampoline reads x2=envc, x3=envp_ptr, populates
`ENV_TABLE` from the forwarded strings, and then seeds the
default `PATH`/`HOME`/`USER`/etc. via `env_init()` (so
caller-supplied values are preserved; defaults only fill in
missing keys).

### File actions

Each `posix_spawn_file_actions_add*` call records a typed
entry in a stack-allocated `posix_spawn_file_actions_t`.  At
spawn time the table is encoded into a third VMO with the
format below and handed to `sys_spawn_std`, which calls
`apply_file_action` against the freshly-spawned child's
fd_table.  The 1.0 subset is `close(fd)` + `dup2(oldfd, newfd)`
with `newfd ∈ {0, 1, 2}`; `addopen` is recorded but the
kernel encodes it as op=0 (no-op) until the path VMO machinery
ships.

### What is NOT yet implemented

* Real `addopen` (recorded but not exercised — see above).
* `POSIX_SPAWN_SETPGROUP` / `POSIX_SPAWN_SETSID` — accepted but
  not applied.  The child inherits the spawner's session in
  1.0.

## Exposed Interfaces

### Flag bits

| Symbol | Value |
|--------|-------|
| `POSIX_SPAWN_RESETIDS` | 1 |
| `POSIX_SPAWN_SETPGROUP` | 2 |
| `POSIX_SPAWN_SETSIGDEF` | 4 |
| `POSIX_SPAWN_SETSIGMASK` | 8 |
| `POSIX_SPAWN_SETSID` | 16 |
| `POSIX_SPAWN_WAITPID` | 0x1000 (CapsuleOS extension) |

### Types

| Symbol | Purpose |
|--------|---------|
| `pub struct posix_spawn_file_actions_t` | Inline record of up to 32 file actions (`addopen` / `addclose` / `adddup2`).  Caller owns the storage; pass-by-pointer in the C ABI. |
| `pub struct posix_spawnattr_t` | Spawn attribute object (flags, pgroup, sigdefault, sigmask).  Caller owns the storage. |

### File actions

| Symbol | Purpose |
|--------|---------|
| `fn posix_spawn_file_actions_init` | Zero-init the object. |
| `fn posix_spawn_file_actions_destroy` | Release any internal state.  Inline in 1.0, so this is a no-op. |
| `fn posix_spawn_file_actions_addopen(actions, newfd, path, oflag, mode)` | Record an `open(path)` redirecting to `newfd`.  Recorded only; the kernel does not yet perform the open. |
| `fn posix_spawn_file_actions_addclose(actions, fd)` | Record a `close(fd)` for the child.  Recorded only. |
| `fn posix_spawn_file_actions_adddup2(actions, oldfd, newfd)` | Record a `dup2(oldfd, newfd)` for the child.  Honoured for `newfd` ∈ {0, 1, 2}; silently no-op for higher fds in 1.0. |

### Attributes

| Symbol | Purpose |
|--------|---------|
| `fn posix_spawnattr_init` | Default-init. |
| `fn posix_spawnattr_destroy` | No-op in 1.0. |
| `fn posix_spawnattr_setflags` | Combine the flag bits above. |
| `fn posix_spawnattr_setpgroup` | Not honoured yet. |
| `fn posix_spawnattr_setsigdefault` | Not honoured yet. |
| `fn posix_spawnattr_setsigmask` | Not honoured yet. |

### Dispatcher

| Symbol | Purpose |
|--------|---------|
| `fn posix_spawn(pid_out, path, file_actions, attrp, argv, envp) -> i32` | Spawn `path` (BootFS lookup).  On success, writes the new pid to `*pid_out` if non-null, and returns 0.  On failure, returns an errno-style code (typically `ENOENT` = 2, `EINVAL` = 22, etc.). |
| `fn posix_spawnp(pid_out, file, file_actions, attrp, argv, envp) -> i32` | Same as `posix_spawn` but uses PATH lookup.  In 1.0 PATH is a fixed string seeded at boot; the function is therefore a synonym for `posix_spawn` once the file name is resolved. |

## Invariants / Caveats

1. `POSIX_SPAWN_WAITPID` makes the call synchronous: the caller
   blocks on `SYSCALL_WAIT4` until the child becomes a zombie.
   This is the CapsuleOS-specific flag; POSIX has no
   equivalent (it uses the caller-side `wait`/`waitpid`).
2. The underlying `SYSCALL_FORK` syscall is **not** used by
   `posix_spawn`.  See `capsule_deprecation.h` for the
   rationale.
3. `addopen` / `addclose` are recorded but not performed by
   the kernel.  Calling them does NOT affect the spawned
   child in 1.0.  This is documented here so callers don't
   silently observe no-ops.
4. Only `newfd` ∈ {0, 1, 2} in `adddup2` is honoured.  A
   caller that tries to dup2 into a higher slot still gets a
   fresh process — the dup2 itself just becomes a no-op.

## Future Work

* Push real `addopen` support into the kernel's spawn path:
  this requires a third VMO carrying the path blob so the
  kernel can complete the `open` against `fileagent`'s IPC
  VFS protocol.  `addclose` already works.
* Honour `POSIX_SPAWN_SETPGROUP` / `POSIX_SPAWN_SETSID` by
  threading the requested session/pgroup into the kernel's
  `process_create` argument.
* Promote `POSIX_SPAWN_SETSIGDEF` / `POSIX_SPAWN_SETSIGMASK`
  to first-class once the signal subsystem has per-process
  state.
* Generalise `adddup2` beyond the std-fd range: this requires
  the kernel to walk an open-fd table inside the child's
  freshly-built container, which currently only carries the
  three std fds.
* End-to-end `posix_spawn` round-trip with a co-operating
  child: the test suite currently has to settle for "syscall
  path is wired" assertions because the only BootFS binaries
  (`ls`, `cat`) are fileagent-bound and don't echo envp.  A
  dedicated `envprint` test binary would close that loop.