#!/usr/bin/env bash
# run-debug-cpu-max.sh -- variant of run-debug.sh that boots QEMU with
# `-cpu max` instead of `-cpu cortex-a72` to test whether the KERNEL_HEALTH.md
# A2 EL0-FAULT at FAR=stack_top-16 reproduces on a different CPU model.
#
# `-cpu max` on `-M virt,secure=off` expands to a Cortex-A57-like core with
# every optional feature enabled.  Cortex-A57 is AArch64v8 baseline + crypto,
# the same memory-model class as the BCM2837 (Cortex-A53) we would target on
# actual Pi Zero 2 WH hardware.  If A2 disappears on `-cpu max` but stays
# on `-cpu cortex-a72`, the root cause is *not* a kernel bug but a QEMU-TCG
# modelling difference for the cortex-a72 / GICv3 / PSCI 1.0 combination.
#
# Usage:
#   pkill -9 qemu-system-aarch64 2>/dev/null
#   bash tools/run-debug-cpu-max.sh
#
# Requires: `gdb` >= 14 (Apple Silicon brew ships 17.x), `qemu-system-aarch64`,
# and a built image at `build/dist/distribution/capsuleos-pangu-1.0.*-aarch64-*.img`.

set -uo pipefail

QEMU_AARCH64="${QEMU_AARCH64:-qemu-system-aarch64}"
GDB="${GDB:-gdb}"
KERNEL_ELF="${KERNEL_ELF:-build/dist/kernel/kernel.elf}"
BOOTLOADER="${BOOTLOADER:-build/target/aarch64-unknown-none/release/capsule-bootloader.bin}"
HNCORE="${HNCORE:-build/dist/kernel/hnxcore}"
DTB="${DTB:-build/dist/qemu.dtb}"
GDB_TIMEOUT="${GDB_TIMEOUT:-60}"
LOG_OUT="${LOG_OUT:-/tmp/gdb-dump-cpu-max.log}"
QEMU_LOG="${QEMU_LOG:-/tmp/qemu-run-cpu-max.log}"

# Pre-req checks (same as run-debug.sh).
for f in "$KERNEL_ELF" "$BOOTLOADER" "$HNCORE" "$DTB"; do
    [ -f "$f" ] || { echo "ERROR: missing $f" >&2; exit 1; }
done
for cmd in "$QEMU_AARCH64" "$GDB"; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: $cmd not on PATH" >&2; exit 1; }
done

echo "[run-debug-cpu-max] Booting QEMU with -cpu max, paused (-s -S) ..."
# Note: only difference from run-debug.sh is `-cpu max`.
"$QEMU_AARCH64" \
    -M virt,secure=off -cpu max -m 512M -nographic \
    -device loader,file="$BOOTLOADER",addr=0x44000000,cpu-num=0,force-raw=on \
    -device loader,file="$HNCORE",addr=0x40700000,force-raw=on \
    -device loader,file="$DTB",addr=0x42000000,force-raw=on \
    -s -S -semihosting >"$QEMU_LOG" 2>&1 &
QEMU_PID=$!

trap 'kill "$QEMU_PID" 2>/dev/null || true; wait "$QEMU_PID" 2>/dev/null || true' EXIT

sleep 0.5

# Same GDB batch as run-debug.sh; the only change is the `echo` banner.
echo "[run-debug-cpu-max] GDB batch session for $GDB_TIMEOUT s, dumping to $LOG_OUT ..."

# SIGALRM-based timeout wrapper (macOS doesn't ship coreutils `timeout`).
run_with_timeout() {
    local secs="$1"; shift
    perl -e '
        use POSIX ":sys_wait_h";
        use POSIX qw(sigaction SIGALRM SIGTERM);
        my $secs = shift @ARGV;
        my $pid = fork();
        if ($pid == 0) { exec(@ARGV); die "exec: $!\n"; }
        local $SIG{ALRM} = sub {
            kill SIGTERM, $pid;
            waitpid($pid, 0);
            exit 124;
        };
        alarm $secs;
        waitpid($pid, 0);
        my $rc = $? >> 8;
        exit $rc;
    ' "$secs" "$@"
}

run_with_timeout "$GDB_TIMEOUT" "$GDB" -q -batch \
    -ex "set architecture aarch64" \
    -ex "set pagination off" \
    -ex "set print pretty on" \
    -ex "file $KERNEL_ELF" \
    -ex "target remote :1234" \
    -ex "break aarch64_sync_el0_handler" \
    -ex "continue" \
    -ex "echo === -cpu max BREAK: first EL0 fault ===" \
    -ex "monitor info registers" \
    -ex "x/16gx 0x404cc000" \
    -ex "x/16gx 0x404e5000" \
    -ex "kill" \
    -ex "quit" >"$LOG_OUT" 2>&1
GDB_EXIT=$?

echo "=== BEGIN cpu-max DUMP (last 80 lines of $LOG_OUT) ==="
tail -n 80 "$LOG_OUT"
echo "=== END cpu-max DUMP (full log at $LOG_OUT) ==="
exit "$GDB_EXIT"
