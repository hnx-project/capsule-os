# 📁 `libc` — CapsuleOS POSIX Compatibility Layer

## Status
[Active (使用中)]

## Name
**`libc`** — `#![no_std]` POSIX / C-ABI standard library that all EL0 user-space
programs link against. Compiled to `libcapsule_libc.rlib` and statically linked into
each program image (no `dlmopen`/`RTLD_*` in 1.0).

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `libcapsule` (syscalls, fd, tty, env, posix_spawn) | Sole downstream handler for the syscall ABI.  `libc` translates POSIX semantics into `libcapsule::syscalls::*` plus the inline IPC patterns documented in DEVELOPMENT.md §5. |
| `libstd` | Uses libc's `errno` + `write_fd` for the `println!` and `eprintln!` macros. |
| `kernel` | Owns the syscall dispatch (`kernel/src/syscall/mod.rs`); the libc symbols below are the C-ABI projection of that dispatch. |
| `shared::status` | Re-exported as `Status`.  Most libc stubs map a `Status` raw code to a libc errno and return `-1` via `set_errno_and_fail`.  Exception: VFS-status-bearing calls preserve the negative `Status::raw()` so legacy tests can introspect directory-create / fs-mkdir outcomes (see `map_vfs_status_to_errno`). |
| `ohlink-format` / `ohlink-linker` | The bitcode is consumed by `ohlink-linker` to produce `OHLINK` rather than ELF.  When the linker sees a `PT_LOAD` with `p_filesz == 0` it now emits an explicit `Bss` segment (L3 commit `fa3904a`), which is what unlocks the zero-init `pub static mut __HNX_ARGV_*` declarations in `lib.rs`. |

## Core Definition

`libc` provides the three pillars of a POSIX boundary:

1. **C-ABI surface** — extern "C" functions with stable symbol names matching
   glibc / musl / FreeBSD conventions so any binary that compiles against
   this crate and links against musl-style headers reaches the kernel via
   the syscalls registered in `kernel/src/syscall/handlers/`.
2. **errno model** — a per-process thread-local `AtomicI32` accessed via
   `errno_get()` / `errno_set()` and the helper `set_errno_and_fail(err)` /
   `set_errno_only(err)`.  Negative `Status::raw()` codes are preserved for
   VFS-bearing syscalls (Tier A commit `2c8de20`).
3. **Init / boot ABI** — the kernel's trampoline writes `argc`, `argv`
   pointer table and argv length table into `__HNX_ARGC`,
   `__HNX_ARGV_PTRS`, `__HNX_ARGV_LENS` (all in BSS) before user code
   runs.  `hnx_argc()`, `hnx_argv()`, `hnx_arg(i)` expose the table.

### What is NOT in `libc`

* No locale data beyond a static `nl_langinfo(CODESET) = "UTF-8"`.
* No dynamic linking (`dlopen`/`dlsym`/`dlerror` all return `ENOSYS`).
* No `mmap` — kernel side is not yet wired; userspace uses the OHLINK
  segment materialised by the loader instead.
* No `*printf` family — stable Rust's `c_variadic` is unstable; bash 5.3
  and readline fall back to their bundled `lib/intl/{,va,s,n,sn,vsn}printf.c`.
* No `fork()` at the ABI boundary — `posix_spawn(3)` is the public
  spawn path; the underlying `SYSCALL_FORK` syscall is **deliberately
  retained** because bash's `JOB_CONTROL` path and procmgr's service
  spawn both go through it.  See
  `libraries/libcapsule/include/capsule_deprecation.h`.

## Module Layout

| File | Surface | Lines (approx) |
|------|---------|----------------|
| `lib.rs` | entry/argv, errno constants, FCNTL/TIOC constants, high-level wrappers (`open_str`, `execve`, `umask`, …) | 1684 |
| `posix_stub.rs` | `errno` AtomicI32 accessor; `set_errno_and_fail`/`set_errno_only`/`errno_value` | 252 |
| `syscalls.rs` | raw `SYSCALL_*` thin wrappers (`exit`, `write_fd`, `wait4`, `sigaction`, `kill`, `pipe`, …) | 184 |
| `env.rs` | `ENV_TABLE` (64 × 96B), `setenv_bytes`, `env_init`, `environ` | 308 |
| `strings.rs` | strsignal, strcasecmp, strncasecmp, memchr, memrchr, strsep, strlcpy, strlcat | 303 |
| `strings_extra.rs` | bcopy, bzero | 53 |
| `ctype.rs` | 13 ASCII classification predicates + tolower/toupper via 256-byte CT_FLAGS table | 203 |
| `posix_gnu.rs` | GNU/POSIX extension surface (~30 funcs: termios, pty, shm, dlsym, getpwuid, …) | 360 |
| `env/API.md` | separate module-level doc | — |

## Exposed Interfaces

### Process / launch / init
| Symbol | File | Purpose |
|--------|------|---------|
| `pub static mut __HNX_ARGC: i32` | lib.rs:50 | argc written by `_hnx_user_entry` trampoline |
| `pub static mut __HNX_ARGV_PTRS: [*const u8; 16]` | lib.rs:55 | argv pointer table (BSS) |
| `pub static mut __HNX_ARGV_LENS: [usize; 16]` | lib.rs:58 | per-arg string lengths |
| `hnx_argc() -> i32` | lib.rs:60 | accessor (safe wrapper) |
| `hnx_argv() -> *const *const u8` | lib.rs:64 | accessor |
| `hnx_arg(i: usize) -> &'static [u8]` | lib.rs:71 | slot accessor (returns `&[]` for OOB) |
| `execve(path, argv) -> i32` | lib.rs:928 | POSIX exec; re-executes entry trampoline |
| `pub static mut __HNX_UMASK: u32` | lib.rs:1361 | process umask (SCO `/sys` style) |
| `posix_spawn*` | `libcapsule::posix_spawn` (re-exported) | Fuchsia-style fresh-container spawn |

### errno
| Symbol | File | Purpose |
|--------|------|---------|
| `errno` (`AtomicI32`) | posix_stub.rs:14 | thread-safe errno cell (Tier A `f7edc59`) |
| `set_errno_and_fail(err) -> i32` | posix_stub.rs:16 | sets errno, returns `-1` |
| `set_errno_only(err)` | posix_stub.rs:24 | sets errno without returning |
| `errno_value() -> i32` | posix_stub.rs:31 | atomic read |
| `errno_get()` / `errno_set(v)` | lib.rs:446 / :451 | safe aliases |
| `EPERM..ERANGE` constants | lib.rs:408-430 | 23 errno constants (Tier A sweep added full coverage) |
| `map_vfs_status_to_errno` | lib.rs (private) | negative Status → (errno, raw Status) preserving the legacy VFS error contract |

### Stdio / fcntl / ioctl
| Symbol | File | Purpose |
|--------|------|---------|
| `F_GETFD`, `F_SETFD`, `F_GETFL`, `F_SETFL`, `F_DUPFD`, `F_DUPFD_CLOEXEC` | lib.rs:1454-1459 | fcntl cmd constants |
| `O_NONBLOCK`, `FD_CLOEXEC` | lib.rs:1462, 1464 | open/fcntl flag constants |
| `TIOCGWINSZ`, `TCGETS`..`TCSETSF`, `TIOCGPGRP`/`TIOCSPGRP`, `TIOCSCTTY`/`TIOCNOTTY`, `TIOCSCTTY_FINDEX` | lib.rs:1466-1475 | ioctl opcodes |
| `close_cloexec_fds() -> i32` | lib.rs:1599 | walks USER_FD_TABLE closing FD_CLOEXEC-marked slots (used on `posix_spawn` / `exec`) |

### Strings / memory
| Symbol | File | Purpose |
|--------|------|---------|
| `strcasecmp` / `strncasecmp` | strings.rs:24, 58 | case-insensitive compare |
| `memchr` / `memrchr` | strings.rs:136, 156 | byte search (GNU `memrchr` is bidirectional) |
| `strsep` | strings.rs:100 | BSD tokenise-in-place |
| `strlcpy` / `strlcat` | strings.rs:184, 217 | bounded copy/concat |
| `strsignal(sig: i32) -> *const u8` | strings.rs:267 | 32-signal static lookup table |
| `bcopy(src, dst, n)` / `bzero(s, n)` | strings_extra.rs | BSD memset variations (vasprintf/snprintf/sprintf deferred — see "What is NOT in `libc`") |

### Ctype
All 13 predicates + 2 case-conversions land on a 256-byte `CT_FLAGS` lookup
table initialised at first call (16-byte aligned BSS) so the heap is never
touched in the hot path:

| Symbol | File | Equivalent glibc symbol |
|--------|------|------------------------|
| `isascii`, `isblank`, `isgraph`, `isprint`, `isspace`, `isxdigit` | ctype.rs | `<ctype.h>` |
| `isdigit`, `isalpha`, `isalnum`, `isupper`, `islower` | ctype.rs | |
| `tolower`, `toupper` | ctype.rs | (case-conversion, table-driven) |

### GNU/POSIX extensions (`posix_gnu.rs`)
bash 5.3 / readline 8.x AC_CHECK_FUNCS probe these on `configure`; everything
listed either does the real thing or returns `ENOSYS`/`EOPNOTSUPP` so the
autoconf probe produces a deterministic answer.  Always-built-in logic in
bash/lib/readline is consulted where autoconf says "no":

| Symbol | Status | Reason |
|--------|--------|--------|
| `cfsetspeed`, `cfmakeraw`, `tcsendbreak` | real no-op | termios is a polling pass-through (no line discipline yet) |
| `openpty` / `forkpty` / `grantpt` / `unlockpt` / `ptsname` / `ptsname_r` / `posix_openpt` | real | routed through `/dev/ptmx` to `tty` service |
| `nl_langinfo(CODESET)` | real, returns `"UTF-8"` | single-locale build |
| `shm_open`, `shm_unlink`, `shm_mkstemp`, `memfd_create` | `ENOSYS` | no shm backend in 1.0 |
| `dlopen`, `dlclose`, `dlsym`, `dlerror` | `ENOSYS` | bash is built `--disable-loadable-builtins` |
| `mkstemp`, `mkdtemp`, `getentropy` | `ENOSYS` | reliant on shm/random backends |
| `getpwuid`, `getpwnam`, `getpwent` | real | single-tenant: returns static `root` |
| `setresuid`, `setresgid`, `getdtablesize` | real | returns `0`/`64` |
| `__argz_count`, `__argz_next`, `__argz_stringify` | real | GNU `<argz.h>` stubs |

### Syscall thin wrappers (`syscalls.rs`)
Every C-ABI symbol that hits a single `SYSCALL_*` number round-trips
through here:

| Symbol | File | Underlying syscall |
|--------|------|---------------------|
| `exit(code)` | syscalls.rs:6 | `SYSCALL_EXIT` |
| `write_fd(fd, ptr, len)` | syscalls.rs:11 | bulk `SYSCALL_WRITE` (4 KiB cap, single safe_copy_from_user) |
| `exec_impl(name)` / `execve_impl(path, argv)` | syscalls.rs:15, 22 | `SYSCALL_EXEC` |
| `getcwd(buf)` / `chdir(path)` | syscalls.rs:51, 68 | `SYSCALL_GETCWD` / `SYSCALL_CHDIR` |
| `wait4(pid, status_ptr, options)` | syscalls.rs:96 | `SYSCALL_WAIT4` (honors `WNOHANG`/`WUNTRACED` from Tier B S6) |
| `getppid()` | syscalls.rs:113 | `SYSCALL_GETPPID` |
| `sigaction` | syscalls.rs:123 | `SYSCALL_SIGACTION` (`SIG_DFL`/`SIG_IGN` only) |
| `raise(sig)` | syscalls.rs:137 | self-targeted signal |
| `kill(pid, sig)` | syscalls.rs:146 | cross-process signal (child-only enforcement) |
| `pause()` | syscalls.rs:155 | `SYSCALL_PAUSE` (uses `thread_sleep`, bounded 4096 iters ~ 70s) |
| `pipe(ufds_ptr)` / `pipe_pair(ufds)` | syscalls.rs:164, 173 | `SYSCALL_PIPE` |
| `dup2(oldfd, newfd)` | syscalls.rs:177 | `SYSCALL_DUP2` |

### ENV table (`env.rs`, with own `env/API.md` doc)
64-slot, 96-byte-per-slot, BSS-backed ENTRY — materialised by the user
entry trampoline (`_hnx_user_entry`) from the `SYSCALL_SPAWN_STD` VMO
payload, then `env_init()` seeds defaults for missing keys.

## Invariants / Caveats

1. **errno-via-`AtomicI32` + Ordering::SeqCst** — every read/write of the
   errno cell goes through the atomic.  No thread-local storage (TLS) in
   1.0 so we share one cell globally per process.  glibc uses thread-local
   `errno`; in single-threaded bash/readline this distinction is
   observationally invisible.
2. **Negative `Status` preserved for VFS-bearing calls** — `mkdir`,
   `mkdir_dup`, `stat`, and other VFS calls return the raw `Status::raw()`
   negative value as the C return so `testall::test_mkdir_dup` (which
   asserts `== -10` for `Status::AlreadyExists`) continues to work.
   Callers that want a plain `-1` + errno pair go through
   `set_errno_and_fail(EEXIST)` instead.
3. **`fork()` is `ENOSYS` at the libc boundary** — `posix_spawn(3)` is
   the public spawn path.  The kernel's `SYSCALL_FORK` is retained for
   internal use by `procmgr` and bash's `JOB_CONTROL` path (bypassing
   libc).  Documented in `libraries/libcapsule/include/capsule_deprecation.h`.
4. **No dynamic linking in 1.0** — programs are statically linked.
   `dlopen`/`dlsym` return `ENOSYS`.  Trying to call them from bash runs
   into the bundled fallback paths bash ships with in `lib/intl/`.
5. **`fcntl(F_GETFL)` round-trips flags via userspace caching** — the
   kernel does not yet store per-fd flags; the libc side keeps a tiny
   cache and returns it.  `O_NONBLOCK` is the only flag with non-trivial
   semantics and is forwarded through `sys_ioctl`/`safe_copy_to_user`.

## Future Work

* **Printf family** — defer until stable Rust's `c_variadic` lands,
  then re-export `asprintf`/`vasprintf`/`snprintf`/`printf`/`vsnprintf`.
  Bash + readline both have bundled fallback implementations so this is
  not a 1.0 blocker.
* **`vasprintf` shim** — alternative: a `vasprintf`-compatible wrapper
  that writes into a fixed 1 KiB stack buffer and copies via
  `safe_copy_to_user`.  Tradeoff: lower fidelity (`%n$` positional args)
  for zero dep on `c_variadic`.
* **`mmap`** — when kernel-side VMO-mapping lands, expose
  `mmap(addr, len, prot, flags, fd, off)` and let `bash`'s heap
  allocator fall back transparently.
* **`getaddrinfo`** — not yet implemented; blocking on the netd
  service's resolver.
* **Wide-character surfaces** — `wcwidth` and friends used by
  readline's display path.  The `wchar_t` table is reserved in
  `libstd`; we only need the readline-facing subset (~20 functions).

## Cross-References

* Tier A `2c8de20` — errno full-coverage sweep
* Tier A `f7edc59` — errno `static mut` → `AtomicI32`
* Tier A `30a4c43` — bulk `sys_write` path
* Tier A `4b7d2b1` — `pause()` thread_sleep + dead code purge
* Tier A `6f050de` — drop WATCH log_error! branches
* Tier A `0c87ac0` — fork path fix + testall baseline 97/97
* Tier B S2.1 `80dd70d` — `strings.rs`
* Tier B S2.2 `6241898` — `ctype.rs`
* Tier B S2.3 `bd17c0c` — `strings_extra.rs`
* Tier B S2.4 `16813fd` — `posix_gnu.rs`
* Tier B S6   `6960835` — `sys_wait4` honors WNOHANG / WUNTRACED
* Tier B L4   `29336c7` — drop `__umask_atomic` hack
* Tier B L3   `fa3904a` — `ohlink-linker` emits explicit Bss segment
