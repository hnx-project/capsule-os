# 📁 `libc::env` — POSIX `getenv / setenv / unsetenv` for `libc`

## Status
[Active (使用中)]

## Name
**`libc::env`** — single-process in-memory environment table seeded during libc init.

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `libc::_hnx_user_entry` | Calls `env_init()` after `__HNX_ARGC` is seeded, so every program boots with `PATH` / `HOME` / `USER` available. |
| `libc::getenv / setenv / unsetenv` | The C-ABI exports that bash / coreutils / osh actually call. They are thin wrappers around the internal `*_bytes` and slot-table routines. |
| `libraries::libcapsule::syscall!` | Not used directly — environment variables are user-space state only. |

## Core Definition
S1 hard-codes a single process's environment into a fixed-size static array. Bash needs `getenv("PATH")`, `getenv("HOME")`, `setenv("VAR", "val")` etc; we seed the array during `env_init()` and let the program mutate it from there.

The table lives entirely in EL0 memory — there is no kernel support and no IPC round-trip. Each `KV` is stored as a `KEY=VALUE\0` blob up to 96 bytes; pointers are rebuilt lazily whenever the table mutates so the `extern char **environ` symbol stays in sync.

## Exposed Interfaces

### Constants
| Symbol | Purpose |
|--------|---------|
| `pub const ENV_SLOTS: usize = 64` | Maximum number of `KEY=VALUE` entries the process can hold. |
| `pub const ENV_KV_BYTES: usize = 96` | Maximum length of a single `KEY=VALUE\0` blob (including the `=` separator and the trailing NUL). |

### Types
| Symbol | Purpose |
|--------|---------|
| `pub struct EnvSlot { pub bytes: [u8; ENV_KV_BYTES], pub used: bool }` | One storage slot. `#[repr(C)]` for static ABI predictability. |

### Storage globals
| Symbol | Purpose |
|--------|---------|
| `pub static mut ENV_TABLE: [EnvSlot; ENV_SLOTS]` | The KV store. `bytes` holds the NUL-terminated `KEY=VALUE` blob; `used` flags a live slot. |
| `pub static mut ENV_PTRS: [*const u8; ENV_SLOTS]` | Pointer array mirroring `ENV_TABLE` for the `extern char **environ` ABI. |
| `pub static mut ENV_COUNT: usize` | Number of live entries. |
| `pub static mut environ: *mut *const u8` | Standard POSIX `environ` pointer; alias for `ENV_PTRS.as_mut_ptr()` after each `env_rebuild_pointers()`. |

### C-ABI exports
| Symbol | Purpose |
|--------|---------|
| `pub unsafe extern "C" fn setenv(key: *const u8, value: *const u8) -> i32` | POSIX `setenv`. |
| `pub unsafe extern "C" fn unsetenv(key: *const u8) -> i32` | POSIX `unsetenv`. |
| `pub unsafe extern "C" fn getenv(key: *const u8) -> *const u8` | POSIX `getenv` — returns a pointer into the matching slot's `bytes` array. |

### Internal helpers
| Symbol | Purpose |
|--------|---------|
| `pub fn setenv_bytes(key: &[u8], value: &[u8]) -> i32` | Used by `env_init` and by `setenv` after copying the C strings into a scratch buffer. |
| `pub fn env_init()` | Seeds the table with `$PATH`, `$HOME`, `$USER` defaults. Called once from `_hnx_user_entry`. |

## Storage layout

```text
ENV_TABLE[i].bytes[0..N]   :=  "KEY=VALUE\0"   (N <= 96, including NUL)
ENV_TABLE[i].used          :=  true iff this slot is live
ENV_PTRS[0..count]         :=  ptrs into ENV_TABLE[i].bytes for live slots
ENV_COUNT                  :=  count
environ                    :=  ENV_PTRS.as_mut_ptr()
```

`env_rebuild_pointers()` is invoked after every `setenv` / `unsetenv` so the `environ` ABI never points into stale slots.

## Invariants / Caveats

1. `ENV_TABLE` is **single-process**, not shared across fork children. The S3 fork path deliberately leaves `ENV_TABLE` empty in the child — the child re-execs through `procmgr`, which re-seeds via `env_init()`.
2. `ENV_KV_BYTES = 96` is a hard cap; setting a `KEY=VALUE` longer than 96 bytes fails with `Status::InvalidArgs` from `setenv`. This covers realistic bash environment strings.
3. `getenv` returns a raw pointer into `ENV_TABLE[i].bytes`. Callers must not free it; the pointer is valid until the next `setenv` / `unsetenv` for that key.

## Future Work

* Move the table into a per-process VMO so fork children automatically inherit / copy-on-write.
* Surface a `putenv` (`NAME=VALUE` style) entry point to match POSIX.