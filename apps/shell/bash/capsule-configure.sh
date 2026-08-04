#!/bin/bash
# 🐚 capsule-configure.sh — cross-compile wrapper for GNU bash 5.3 on CapsuleOS
#
# This is a STUB. It documents the flags the next-session xtask integration
# will use when actually building bash for the aarch64-unknown-capsule
# triple.  It is intentionally NOT executable in this commit so we can
# iterate on the configure invocation without breaking the regular
# `xtask code build --arch aarch64` flow.
#
# Future usage (from xtask build_foreign_subproject):
#
#   cd userspace/posix/bash
#   ./capsule-configure.sh
#   make -j$(nproc)
#   # ohlink-linker then packs ./bash into staging_rootfs/system/bin/bash
#
# Manual debugging:
#
#   ./capsule-configure.sh --debug   # sets -x and keeps config.log
#
set -euo pipefail

PROG_NAME="capsule-configure.sh"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# ---------------------------------------------------------------------------
# Cross-toolchain detection.  In an ideal world these come from rustup's
# `+nightly` environment (rust-lld, llvm-objcopy, ...); today we expect the
# capsule target spec to encode enough hints, so a plain `cc` is fine for
# bash's C sources.
# ---------------------------------------------------------------------------

# TODO(next session): derive these from `rustc -Z json-target-spec` or a
# pinned aarch64-unknown-capsule-gcc wrapper.
: "${CC:=aarch64-unknown-capsule-gcc}"
: "${AR:=aarch64-unknown-capsule-ar}"
: "${RANLIB:=aarch64-unknown-capsule-ranlib}"
: "${LD:=aarch64-unknown-capsule-ld}"

export CC AR RANLIB LD

# ---------------------------------------------------------------------------
# bash 5.3 doesn't recognise `aarch64-unknown-capsule` out of the box; its
# config.sub rejects unknown vendors.  We pre-populate the cache vars that
# bash's configure probes so it doesn't run those probes at all (they
# would fail anyway because the probe binaries can't run on the host).
# ---------------------------------------------------------------------------

CACHE_VARS=(
    # bash 5.3's `ac_sys_largefile` probe tries to build a 64-bit binary;
    # we know we want largefile on a 64-bit target.
    ac_cv_sys_file_offset_bits=64
    ac_cv_sys_large_files=1

    # bash probes `/dev/fd/` via open(); on CapsuleOS this isn't
    # backed by the fileagent service yet.  Skip the probe.
    bash_cv_dev_fd=whence

    # bash probes for `sysconf(_SC_MAPPED_FILES)` to decide on mmap use.
    # We don't have mmap in 1.0, so tell bash's configure it does not.
    bash_cv_mmap=yes     # actually: mmap=no once we wire it
)

CACHE_ARGS=()
for kv in "${CACHE_VARS[@]}"; do
    CACHE_ARGS+=(--with-cache-vars="$kv")
done

# ---------------------------------------------------------------------------
# Configure invocation (planned).  Real call below.
# ---------------------------------------------------------------------------

CONFIGURE_ARGS=(
    --host=aarch64-unknown-capsule
    --target=aarch64-unknown-capsule
    --disable-nls
    --without-bash-malloc
    --disable-loadable-builtins
    --enable-static-link
    --with-installed-readline=no
    --with-included-gettext
    "${CACHE_ARGS[@]}"
)

if [[ "${1:-}" == "--debug" ]]; then
    set -x
    KEEP_LOG=1
else
    KEEP_LOG=0
fi

cat <<INFO
🐚 $PROG_NAME — GNU bash 5.3 cross-compile for CapsuleOS
  cwd       : $SCRIPT_DIR
  host      : aarch64-unknown-capsule
  CC        : $CC
  AR        : $AR
  RANLIB    : $RANLIB
  cache vars: ${CACHE_VARS[*]}
INFO

# ---------------------------------------------------------------------------
# STUB: the real call is disabled so this commit doesn't run autotools.
# Uncomment the next line when the xtask pipeline + cross-cc are in place.
# ---------------------------------------------------------------------------

# ./configure "${CONFIGURE_ARGS[@]}"

echo
echo "🛑 $PROG_NAME: stub mode — configure NOT executed."
echo "   Uncomment the './configure ...' line above once xtask+cc are wired."
echo "   See README.capsule.md for the rationale."
exit 0