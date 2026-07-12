#!/usr/bin/env bash
# run-debug.sh -- one-shot QEMU pause + GDB batch + crash dump.
#
# Wrapper around the manual QEMU `-s -S` + `gdb-multiarch` /
# `gdb --target=aarch64-elf` dance.  Boots QEMU in the background
# paused at the reset vector, then runs a 60s batch GDB session
# against `localhost:1234` that:
#
#   1. Sets architecture = aarch64 (gdb on macOS auto-detects on
#      ELF header; explicit is just belt-and-braces).
#   2. Sets pretty printing + sets pagination off.
#   3. Places three breakpoints:
#        - `launch_user_program_with_argv` (sys_spawn entry)
#        - `kernel/src/task/process.rs:514`
#          (the `set_ttbr0(old_ttbr0)` swap-back site)
#        - `kernel/src/arch/aarch64/trap.rs:120`
#          (the synchronous-EL0 fault entry)
#   4. Continues; on the first breakpoint hit, dumps the
#      registers that KERNEL_HEALTH.md A2 needs to see:
#          $esr_el1 $far_el1 $elr_el1 $spsr_el1
#          $ttbr0_el1 $ttbr1_el1
#          x0 x1 x2 x29 x30 sp_el0 sp
#          disassemble $elr_el1, +32     (the faulting instruction)
#          monitor info registers         (QEMU's full snapshot)
#   5. Kills the QEMU background job, returns.
#
# Output: stdout *and* `/tmp/gdb-dump.log` (full trace).
#
# Usage: bash tools/run-debug.sh
#
# Pre-req:
#   - `gdb` 14+ (Apple Silicon brew ships 17.x) on PATH
#   - `qemu-system-aarch64` on PATH
#   - `cargo xtask code build --arch aarch64` already ran
#     (so build/dist/kernel/kernel.elf exists)

set -uo pipefail

QEMU_AARCH64="${QEMU_AARCH64:-qemu-system-aarch64}"
GDB="${GDB:-gdb}"
KERNEL_ELF="${KERNEL_ELF:-build/dist/kernel/kernel.elf}"
BOOTLOADER="${BOOTLOADER:-build/target/aarch64-unknown-none/release/capsule-bootloader.bin}"
HNCORE="${HNCORE:-build/dist/kernel/hnxcore}"
DTB="${DTB:-build/dist/qemu.dtb}"
GDB_TIMEOUT="${GDB_TIMEOUT:-60}"
LOG_OUT="${LOG_OUT:-/tmp/gdb-dump.log}"
QEMU_LOG="${QEMU_LOG:-/tmp/qemu-run.log}"

# --- 0. Sanity: pre-req files ---
for f in "$KERNEL_ELF" "$BOOTLOADER" "$HNCORE" "$DTB"; do
    if [ ! -f "$f" ]; then
        echo "ERROR: missing prerequisite file: $f" >&2
        echo "       Run 'cargo xtask code build --arch aarch64' first." >&2
        exit 1
    fi
done
for cmd in "$QEMU_AARCH64" "$GDB"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "ERROR: missing tool on PATH: $cmd" >&2
        exit 1
    fi
done

# --- 1. Boot QEMU paused at reset vector ---
echo "[run-debug] Booting QEMU paused (-s -S) and starting gdbserver :1234 ..."
"$QEMU_AARCH64" \
    -M virt,secure=off -cpu cortex-a72 -m 512M -nographic \
    -device loader,file="$BOOTLOADER",addr=0x44000000,cpu-num=0,force-raw=on \
    -device loader,file="$HNCORE",addr=0x40700000,force-raw=on \
    -device loader,file="$DTB",addr=0x42000000,force-raw=on \
    -s -S -semihosting >"$QEMU_LOG" 2>&1 &
QEMU_PID=$!

# Trap so the QEMU background job is always cleaned up,
# even if gdb explodes mid-session.
trap 'kill "$QEMU_PID" 2>/dev/null || true; wait "$QEMU_PID" 2>/dev/null || true' EXIT

# Give QEMU a moment to bind :1234 before gdb connects.
sleep 0.5

# --- 2. GDB batch session ---
echo "[run-debug] GDB batch session for $GDB_TIMEOUT s, dumping to $LOG_OUT ..."

# macOS brew's coreutils doesn't always ship `timeout`; wrap
# gdb in a SIGALRM-based timer ourselves so the script is
# portable.  We use perl because every macOS ship has it
# and it has a clean signal-based process timeout.
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
    -ex "echo === BREAKPOINT 1: First EL0 fault ===" \
    -ex "monitor info registers" \
    -ex "echo === read FAR physical address page ===" \
    -ex "monitor info registers" \
    -ex "x/16gx 0x404e5000" \
    -ex "monitor x/16gx 0x404cc000" \
    -ex "echo === END WALK ===" \
    -ex "kill" \
    -ex "quit" >"$LOG_OUT" 2>&1
GDB_EXIT=$?

# Show the tail of the dump on stdout so we don't have to
# `cat /tmp/gdb-dump.log` after running.
echo "=== BEGIN GDB DUMP (last 80 lines) ==="
tail -n 80 "$LOG_OUT"
echo "=== END GDB DUMP (full log at $LOG_OUT) ==="
exit "$GDB_EXIT"
