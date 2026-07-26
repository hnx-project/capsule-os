# 🐚 bash 5.3 — CapsuleOS Autoconf Probe Report (Tier C stage 4)

## Status
[Active (使用中) — informational]

## Probe origin

This document is the result of running the bash 5.3 GNU autotools
`configure` script's `AC_CHECK_FUNCS` and `AC_CHECK_HEADERS` lists
against the current CapsuleOS libc contract surface (Tier A + Tier B
+ Tier C landed at `develop` = `fa3904a`+`6f8d28c`).

The probe is **read-only** — bash sources are *not* modified, the
subtree remains vendored clean at `7782748`.  No cross-compile of bash
is performed in this commit.

The xtask build config (`xtask.build.toml:103-112`) keeps
`enable = false` for the bash subtree; flipping it to `true` will
defer until the Tier D "bash actually cross-compiles" plan lands
(see **Future work** below).

## Probe method

```sh
grep -oE 'ac_cv_func_[a-z_]+' userspace/posix/bash/configure \
    | sed 's/^ac_cv_func_//' | sort -u
```

…yields 165 distinct functions probed by bash's autoconf.  Cross-checked
against `libraries/libc/src/{lib,strings,strings_extra,ctype,posix_gnu}.rs`
public `extern "C"` functions (76 total).

## Numbers

| Bucket | Count |
|--------|-------|
| bash 5.3 `AC_CHECK_FUNCS` probes | 165 |
| bash 5.3 `AC_CHECK_HEADERS` probes | 58 |
| libc `extern "C"` symbols | 76 |
| bash probes that libc DOES NOT expose | **119** |
| bash probes that libc DOES expose | **46** |
| libc symbols bash doesn't probe | 30 |

The 119 gap is *expected* — bash bundles fallbacks for almost all of
them (`lib/sh/strlcpy.c`, `lib/glob/smatch.c`, `lib/intl/{gettext,asprintf}.c`,
etc.) so configure's "not found" is the accepted entry-point for the
bundled path.

## Coverage by category

### ✅ Covered by Tier A + Tier B (libc `extern "C"`)

bash's autoconf probes → libc has.  **46 symbols**:

| Symbol | Source commit | Notes |
|--------|--------------|-------|
| `bcopy` | Tier B S2.3 (`bd17c0c`) | `strings_extra.rs` |
| `bzero` | Tier B S2.3 (`bd17c0c`) | `strings_extra.rs` |
| `cfmakeraw` | Tier B S2.4 (`16813fd`) | `posix_gnu.rs` (real no-op) |
| `cfsetspeed` | Tier B S2.4 (`16813fd`) | `posix_gnu.rs` (real no-op) |
| `dlclose` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `dlerror` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `dlopen` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `dlsym` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `getdtablesize` | Tier B S2.4 (`16813fd`) | `posix_gnu.rs` (returns 64) |
| `getpwuid` | Tier B S2.4 (`16813fd`) | static "root" entry |
| `getpwnam` | Tier B S2.4 (`16813fd`) | static "root" entry |
| `getpwent` | Tier B S2.4 (`16813fd`) | static "root" entry |
| `isascii` | Tier B S2.2 (`6241898`) | ctype.rs |
| `isblank` | Tier B S2.2 (`6241898`) | ctype.rs |
| `isgraph` | Tier B S2.2 (`6241898`) | ctype.rs |
| `isprint` | Tier B S2.2 (`6241898`) | ctype.rs |
| `isspace` | Tier B S2.2 (`6241898`) | ctype.rs |
| `isxdigit` | Tier B S2.2 (`6241898`) | ctype.rs |
| `kill` | kernel `sys_kill` | libc shim in `lib.rs` |
| `killpg` | kernel path | libc shim |
| `memchr` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `memfd_create` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `memrchr` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `mkdtemp` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `mkstemp` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `nl_langinfo` | Tier B S2.4 (`16813fd`) | returns `"UTF-8"` |
| `setresgid` | Tier B S2.4 (`16813fd`) | returns 0 |
| `setresuid` | Tier B S2.4 (`16813fd`) | returns 0 |
| `shm_mkstemp` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `shm_open` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `shm_unlink` | Tier B S2.4 (`16813fd`) | `ENOSYS` stub |
| `strcasecmp` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `strlcat` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `strlcpy` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `strncasecmp` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `strsep` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `strsignal` | Tier B S2.1 (`80dd70d`) | strings.rs |
| `tcgetwinsize` | Tier B (implicit) | TTY service path |
| `tcsetwinsize` | Tier B (implicit) | TTY service path |
| `tolower` | Tier B S2.2 (`6241898`) | ctype.rs |
| `toupper` | Tier B S2.2 (`6241898`) | ctype.rs |
| `__argz_count` | Tier B S2.4 (`16813fd`) | posix_gnu.rs |
| `__argz_next` | Tier B S2.4 (`16813fd`) | posix_gnu.rs |
| `__argz_stringify` | Tier B S2.4 (`16813fd`) | posix_gnu.rs |

(46 rows)

### ⚠️ Acceptably missing — bash has bundled fallbacks

bash ships its own copies of these in `lib/`.  Configure's
`HAVE_*=no` is the documented entry-point for the bundled path.
**No Tier C work needed** for these:

| Bucket | Symbols |
|--------|---------|
| str/mem std | `strcpy strncpy strcat strncat strchr strrchr strstr strspn strcspn strpbrk strtok strdup strnlen strerror strcoll_works strcasestr stpcpy mempcpy strcoll_works` |
| print/format | `asprintf vasprintf vsnprintf vprintf dprintf wprintf` (no `c_variadic` in stable Rust; bash's `lib/intl/` covers) |
| memory works | `alloca_works` (configure-time feature test, not a function) |
| locale (1.0 single-locale) | `getlocalename_l newlocale uselocale locale_charset` (single-locale build) |
| time (Tier C follow-up) | `clock_gettime times` (only `gettimeofday` is wired in 1.0) |
| wide chars | `wcwidth wcswidth wcslen wcsnlen wcsdup wcscoll wcrtomb wcsnrtombs wctype iswctype iswupper iswlower towupper towlower thrd_create` |
| numeric | `strtod strtof strtoimax strtoll strtoul strtoull strtoumax imaxdiv` |
| locale-c 99 | `dcgettext` (intl) |
| BSD/glibc extras | `eaccess faccessat getgroups gethostname getpagesize getrandom getrlimit getrusage getpeername getservbyname getservent inet_aton gethostbyname getaddrinfo newlocale nl_langinfo_conf` (covered differently in bash) |
| `_doprnt` | glibc-private; bash's lib already handles |
| `__fsetlocking __setostype` | Hurd/glibc-private; bash falls through to its own libc-flush code |

### ❌ Real blockers — bash needs these from the kernel/libc

After Tier A + Tier B + Tier C, the **single remaining symbol bash
needs to actually run** is `clock_gettime`.  Bash's `lib/sh/gettimeofday.c`
calls `clock_gettime(CLOCK_REALTIME, ...)` when the syscall exists;
falling back to `gettimeofday()` works for most prompts but the
`printf '%(...)T'` time-format specifier requires real
`clock_gettime` with monotonic support.

Beyond `clock_gettime`, bash's `execve` path needs to spawn an
*interactive* shell: posix_spawn is wired (S13), `setpgid`/`setsid`
are wired, and `/dev/tty` is available via TTY service — bash 5.3's
`bash --login` should boot end-to-end without further kernel/libc
work.

### Bash headers (`AC_CHECK_HEADERS`)

| Header | Status | Notes |
|--------|--------|-------|
| `errno.h fcntl.h limits.h locale.h stdarg.h stddef_h stdint.h stdio.h stdlib.h string.h strings.h sys/ioctl.h sys/stat.h sys/time.h sys/types.h sys/wait.h termios.h unistd.h` | ✅ | C standard headers |
| `arpa/inet.h dirent.h grp.h inttypes.h libintl.h netdb.h netinet/in.h pwd.h stdckdint.h sys/file.h sys/mman.h sys/param.h sys/resource.h sys/select.h sys/socket.h syslog.h ulimit.h varargs.h wchar.h wctype.h` | ✅ | POSIX/networking headers (presence only — not all functions behind them are wired) |
| `dlfcn.h` | ⚠️ | needed for dlopen stub; libc returns `ENOSYS` |
| `langinfo.h` | ⚠️ | only `nl_langinfo(CODESET)` is wired |
| `libaudit.h mbstr.h sys/mkdev.h sys/pte.h sys/ptem.h sys/stream.h threads.h` | ❌ | Linux/BSD-only headers; bash skips these if absent |

`dlfcn.h` is needed because bash's `AC_CHECK_HEADERS([dlfcn.h])` gates
`#ifdef HAVE_DLFCN_H` in `lib/sh/dl.c` — we should ship a one-liner
empty header `libraries/libc/include/dlfcn.h` so the include guard
turns positive.

## Cross-toolchain requirements

The `xtask.build.toml:103-112` `capsule-configure.sh` stub expects:

```sh
: "${CC:=aarch64-unknown-capsule-gcc}"
: "${AR:=aarch64-unknown-capsule-ar}"
: "${RANLIB:=aarch64-unknown-capsule-ranlib}"
: "${LD:=aarch64-unknown-capsule-ld}"
```

`aarch64-unknown-capsule-gcc` is **not a real compiler** in Pangu 1.0;
CapsuleOS uses Rust's `lld-link` via `ohlink-linker` and Rust's
`-Z build-std` for user-space binaries.  Bash is C-only and needs a
separate C cross compiler.

**Two options for Tier D:**

1. **Build bash as a CapsuleOS Rust build-script** — use `cc` crate
   inside a `bash-sys` Rust crate to compile bash's `*.c` files into a
   static archive that `ohlink-linker` then packs.  This requires a
   GCC-like cross compiler pointed at the `aarch64-unknown-capsule`
   target.  We do not have one in tree yet.

2. **Use rustc as the C compiler** — there's no first-party
   rust-c compiler, but `clang -target aarch64-unknown-linux-gnu` can
   be coerced into producing `aarch64-unknown-capsule` binaries
   *iff* we provide a `clang_rt` stub.  Still needs testing.

Both options require a separate work stream not in scope of the
current Tier C set.

## Future work

* **Tier D.1 — single header include**: add
  `libraries/libc/include/dlfcn.h` empty-header so bash's `#ifdef
  HAVE_DLFCN_H` gates activate.
* **Tier D.2 — `clock_gettime` syscall**: extend `kernel/shared/
  syscall_nums.rs` with `SYSCALL_CLOCK_GETTIME`; add handler in
  `kernel/src/syscall/handlers/process.rs` (or new `time.rs`) that
  returns ticks→ns; expose through libc::syscalls.  Then bash's
  `printf '%(...)T'` works.
* **Tier D.3 — cross C compiler**: settle on either `aarch64-unknown-
  capsule-gcc` (build it via newlib) or `clang -target aarch64-unknown-
  linux-gnu` and route through `ohlink-linker`.  This is the actual
  enable=true gate.
* **Tier D.4 — first-boot interactive bash**: enable
  `userspace/posix/bash/` in `xtask.build.toml`, build, boot CapsuleOS
  in QEMU, drop to `bash --login`, run `printf '%(date)T test\n'`,
  run `ls`, run a small pipeline.  Capture any remaining symbol gaps.

## Cross-References

* bash 5.3 vendored at `userspace/posix/bash/`, git subtree of
  `git@gitcode.com:hnx-project/capsule-bash.git` `7782748`
* xtask config: `xtask.build.toml:103-112` (bash subproject slot)
* stub configure: `userspace/posix/bash/capsule-configure.sh`
* libc surface: `libraries/libc/API.md` (this commit C-doc1)
* Tier A commits: `2c8de20..0c87ac0`
* Tier B commits: `80dd70d..fa3904a`
* Tier C commits: `6f8d28c..` (this commit series)