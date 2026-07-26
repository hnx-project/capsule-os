# `libc` Changelog

Tracks changes to the `libc` runtime contract crate.  Cross-references the
relevant commits in `git log` via the `commit:` markers.  Major-version
bumps (Pangu releases) are recorded in `/CHANGELOG.md` at repo root;
this file is the **libc-only** sub-changelog.

## Unreleased

_No active changes yet._

---

## Tier B (Pangu 1.0 — kernel ABI dispatched at SYSCALL_* boundary)  (commit `0c87ac0..fa3904a`)

### S2 — POSIX/GNU extension surface (bash / readline autoconf probes)

* `feat(libc): strings.rs` (commit `80dd70d`) — `strsignal`,
  `strcasecmp`, `strncasecmp`, `memchr`, `memrchr`, `strsep`, `strlcpy`,
  `strlcat`.  32-signal static lookup, GNU `memrchr` reverse-direction,
  BSD bounded `strlcpy`/`strlcat`.
* `feat(libc): ctype.rs` (commit `6241898`) — 13 ASCII classification
  predicates + `tolower`/`toupper`.  256-byte `CT_FLAGS` lookup table,
  no Unicode coercion.
* `feat(libc): strings_extra.rs` (commit `bd17c0c`) — `bcopy`, `bzero`.
  `*printf` family **deferred** to a future version once stable Rust's
  `c_variadic` lands.
* `feat(libc): posix_gnu.rs` (commit `16813fd`) — termios (`cfmakeraw`,
  `tc*`), `openpty`/`forkpty`/`grantpt`/`posix_openpt`, `nl_langinfo`,
  `dlopen` stub (`ENOSYS`), `shm_open`/`memfd_create` stub, `mkstemp`
  stub, `getpwuid` static, `setresuid(0)`, `__argz_*`.  bash 5.3
  bundles lib/intl fallback when stubs fire.

### S6 — `wait4` correctness
* `feat(kernel): sys_wait4 honors WNOHANG + accepts WUNTRACED` (commit
  `6960835`) — `WNOHANG=1` returns `Ok(0)` when no zombie is queued;
  `WUNTRACED=2` accepted as no-op for 1.0 ABI compat.

### L3 + L4 — code hygiene

* `refactor(libc): drop __umask_atomic hack` (commit `29336c7`) — back
  to `static mut __HNX_UMASK: u32` (`AtomicU32` brought nothing that
  single-threaded umask(2) needs).
* `feat(ohlink): emit explicit Bss segment for p_filesz == 0 PT_LOAD`
  (commit `fa3904a`) — `ohlink-linker` now produces an explicit OHLK
  `Bss` segment (`SegmentType = 4`) so zero-init globals land in mapped
  memory at process launch.  `__HNX_ARGV_PTRS`/`__HNX_ARGV_LENS` no
  longer need `0xDEAD_BEEF` sentinel pre-fills.

---

## Tier A (Pangu 1.0 hardening — errno + bulk write + signal handle path) (commit `2c8de20..0c87ac0`)

### Errno atomicity

* `fix(libc): errno 全量补齐` (commit `2c8de20`) — every C-ABI failure
  path now passes through `set_errno_and_fail(err)` and `errno::EINVAL`
  is imported into the global namespace.  23 errno constants
  (`EPERM..ERANGE`) live in `lib.rs:408-430`.  `map_vfs_status_to_errno`
  added for VFS-bearing calls so the legacy `-10` raw value
  (`Status::AlreadyExists`) survives for `test_mkdir_dup`.
* `refactor(libc): errno → AtomicI32` (commit `f7edc59`) — `errno` is
  no longer `static mut i32`.  `Ordering::SeqCst` everywhere.

### Hot paths

* `perf(kernel): sys_write bulk path` (commit `30a4c43`) — single
  `safe_copy_from_user` call up to 4 KiB.  Halves per-byte
  per-syscall overhead for `println!` heavy programs.
* `fix(kernel): pause() → thread_sleep` (commit `4b7d2b1`) — bounded
  iteration (4096 iters ~ 70s) instead of busy spin-yield.
* `fix(kernel): fork fd_table clone stub` (commit `0c87ac0`) — child
  starts with three reserved slots (fd 0..2) so `posix_spawn` after
  `fork + exec` does not leak parent channels.  Deleted dead
  `_find_process_user_unused` / `deep_clone_handles_and_fds`.
* `chore(kernel): drop WATCH log_error!` (commit `6f050de`) — stale
  write-watch detection branches removed from `safe_copy_from_user`
  / `safe_copy_to_user`.  After Tier A all user-buffer paths use the
  one-shot `safe_copy_*` helpers.

### Test regression floor

* `tools/xtask/src/test.rs` `min_expected_passes = 96` after Tier A
  baseline (97 actual).  Floor reserved so a future `testall` that
  quietly grows by one assertion doesn't require ceremony.
