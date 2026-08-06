//! Kernel log ring buffer (dmesg equivalent).
//!
//! Thread-safe circular buffer with sequence-numbered records.
//! Inspired by Linux's `log_buf` and FreeBSD's `msgbuf`.
//!
//! Records are always appended, even when the console is suppressed.
//! Users read them via the `SYS_LOGBUF_READ` syscall (see
//! `syscall/handlers/logbuf.rs`).

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::kcore::logging::{get_log_level, ConsoleWriter, LEVEL_DEBUG, LEVEL_ERROR, LEVEL_INFO, LEVEL_WARN};

// ----------------------------------------------------------------------------
// Capacity
// ----------------------------------------------------------------------------

/// Payload byte budget shared by every record. Each record embeds its
/// `target` tag (≤ 8 bytes) and a UTF-8 message (≤ `MSG_MAX` bytes).
const MSG_MAX: usize = 200;

/// Maximum number of records held in the ring at once. Once we wrap we
/// silently overwrite the oldest; readers detect gaps via `seq` jumps.
const REC_SLOTS: usize = 256;

/// Total ring buffer byte budget.  ~51 KiB payload + slot metadata.
/// Picked to comfortably hold a full boot session (~700 INFO lines).
pub const LOG_BUF_SIZE: usize = 64 * 1024;

// ----------------------------------------------------------------------------
// Record layout (POD, fixed size — no allocation in EL1)
// ----------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct LogRecord {
    seq: u64,
    timestamp_ticks: u64,
    level: u8,
    target_len: u8,
    msg_len: u16,
    target: [u8; 8],
    msg: [u8; MSG_MAX],
}

impl LogRecord {
    const fn empty() -> Self {
        Self {
            seq: 0,
            timestamp_ticks: 0,
            level: LEVEL_INFO,
            target_len: 0,
            msg_len: 0,
            target: [0; 8],
            msg: [0; MSG_MAX],
        }
    }
}

// ----------------------------------------------------------------------------
// LogBuf — data only. Access is serialised by the spin lock in `LOGBUF`.
// ----------------------------------------------------------------------------

struct LogBuf {
    slots: [LogRecord; REC_SLOTS],
    head: usize,           // next slot to write
    dropped: u64,          // count of overwritten records
    next_seq: u64,         // monotonically increasing sequence number
}

impl LogBuf {
    const fn new() -> Self {
        Self {
            slots: [const { LogRecord::empty() }; REC_SLOTS],
            head: 0,
            dropped: 0,
            next_seq: 0,
        }
    }
}

/// Mutex guarding `LOGBUF`.  Kept as a `static mut AtomicUsize` so
/// callers can use it without dragging in a `Mutex<T>` (which is not
/// const-friendly with our large inner type).
static LOGBUF_LOCK: AtomicUsize = AtomicUsize::new(0);

fn logbuf_lock() {
    while LOGBUF_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn logbuf_unlock() {
    LOGBUF_LOCK.store(0, Ordering::Release);
}

// ----------------------------------------------------------------------------
// Global instance — kept in a mutable static.  Every access goes through
// the spin lock; the `unsafe { &mut LOGBUF … }` wrappers centralise the
// `unsafe` so the macros stay clean.
// ----------------------------------------------------------------------------

pub static mut LOGBUF: LogBuf = LogBuf::new();

/// Convenience wrapper used by the `log_*!` macros.
pub fn log_to_ring(level: u8, target: &str, msg: &str) {
    logbuf_lock();
    // Safety: holds the spin lock, so concurrent writers are excluded.
    let buf = unsafe { &mut LOGBUF };
    let slot_idx = buf.head;
    let next = (slot_idx + 1) % REC_SLOTS;
    buf.head = next;
    if next == 0 {
        buf.dropped = buf.dropped.wrapping_add(1);
    }

    let seq = buf.next_seq.wrapping_add(1);
    buf.next_seq = seq;

    let timestamp_ticks = crate::drivers::timer::get_ticks();

    let target_bytes = target.as_bytes();
    let target_len = core::cmp::min(target_bytes.len(), 8);

    let msg_bytes = msg.as_bytes();
    let msg_len = core::cmp::min(msg_bytes.len(), MSG_MAX);

    let mut rec = LogRecord::empty();
    rec.seq = seq;
    rec.timestamp_ticks = timestamp_ticks;
    rec.level = level;
    rec.target_len = target_len as u8;
    rec.msg_len = msg_len as u16;
    rec.target[..target_len].copy_from_slice(&target_bytes[..target_len]);
    rec.msg[..msg_len].copy_from_slice(&msg_bytes[..msg_len]);

    buf.slots[slot_idx] = rec;

    logbuf_unlock();
}

/// Drain records with `seq > since_seq` into `out`, oldest first.
/// Returns `(newest_seq_seen, bytes_written)`.  `min_level` filters
/// out DEBUG records (3) by default — useful for replay so we don't
/// flood the console with per-CPU TTBR0 debug spam from
/// boot.  `dmesg` (the user-mode tool) passes `LEVEL_DEBUG` to get
/// everything.
pub fn drain_to(since_seq: u64, out: &mut [u8]) -> (u64, usize) {
    drain_to_with_level(since_seq, out, LEVEL_INFO)
}

/// Variant of `drain_to` that exposes the level filter.
pub fn drain_to_with_level(since_seq: u64, out: &mut [u8], min_level: u8) -> (u64, usize) {
    logbuf_lock();
    let (cursor, n) = {
        // Safety: spin lock held.
        let buf = unsafe { &LOGBUF };
        let head = buf.head;
        let mut cursor = since_seq;
        let mut written = 0usize;
        let mut i = 0usize;
        while i < REC_SLOTS && written < out.len() {
            let idx = (head + i) % REC_SLOTS;
            let rec = &buf.slots[idx];
            if rec.seq <= since_seq || rec.seq == 0 {
                i += 1;
                continue;
            }
            // Level filter: DEBUG-only records are skipped during
            // replay so we don't drown the console in TTBR0 churn.
            // Note we use `> min_level` because the kernel encodes
            // levels as 0=ERROR .. 3=DEBUG.
            if rec.level > min_level {
                i += 1;
                continue;
            }

            let level_ch = match rec.level {
                LEVEL_ERROR => b'E',
                LEVEL_WARN => b'W',
                LEVEL_INFO => b'I',
                LEVEL_DEBUG => b'D',
                _ => b'?',
            };

            let s = format_u64(rec.seq);
            let needed = s.len() + 1 + 1 + 1 + rec.target_len as usize + 2 + rec.msg_len as usize + 1;
            if written + needed > out.len() {
                break;
            }

            for &b in s.as_bytes() {
                out[written] = b;
                written += 1;
            }
            out[written] = b' ';
            written += 1;

            out[written] = level_ch;
            written += 1;

            out[written] = b' ';
            written += 1;

            for k in 0..rec.target_len as usize {
                out[written] = rec.target[k];
                written += 1;
            }

            out[written] = b':';
            written += 1;
            out[written] = b' ';
            written += 1;

            for k in 0..rec.msg_len as usize {
                out[written] = rec.msg[k];
                written += 1;
            }

            out[written] = b'\n';
            written += 1;

            cursor = rec.seq;
            i += 1;
        }
        (cursor, written)
    };
    logbuf_unlock();
    (cursor, n)
}

/// Return the next sequence number that will be assigned.
pub fn tail_seq() -> u64 {
    logbuf_lock();
    let seq = unsafe { LOGBUF.next_seq };
    logbuf_unlock();
    seq
}

// ----------------------------------------------------------------------------
// fmt::Write adapter for the `log_*!` macros
// ----------------------------------------------------------------------------

/// Adapter that captures formatted bytes into a stack buffer then forwards
/// to both the console (if allowed) and the ring buffer.
pub struct LogCapture<'a> {
    pub level: u8,
    pub target: &'a str,
    /// If `Some` the formatted message is emitted to the console with the
    /// supplied ANSI escape prefix (matches Linux's coloured `printk`).
    /// `None` skips the console path entirely; the ring buffer still gets
    /// the message.
    pub color_prefix: Option<&'a str>,
    buf: [u8; MSG_MAX],
    len: usize,
}

impl<'a> LogCapture<'a> {
    pub fn new(level: u8, target: &'a str, color_prefix: Option<&'a str>) -> Self {
        Self {
            level,
            target,
            color_prefix,
            buf: [0u8; MSG_MAX],
            len: 0,
        }
    }
}

impl<'a> Write for LogCapture<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let remaining = MSG_MAX - self.len;
        let take = core::cmp::min(bytes.len(), remaining);
        self.buf[self.len..self.len + take].copy_from_slice(&bytes[..take]);
        self.len += take;
        Ok(())
    }
}

impl<'a> Drop for LogCapture<'a> {
    fn drop(&mut self) {
        // Safety: see `log_to_ring`.
        let msg = unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) };
        log_to_ring(self.level, self.target, msg);

        let boot_phase = crate::kcore::logging::boot_phase();
        let allowed_by_phase = match boot_phase {
            0 | 1 => true,
            _ => self.level <= LEVEL_WARN,
        };
        if allowed_by_phase && get_log_level() >= self.level {
            if let Some(prefix) = self.color_prefix {
                let mut w = ConsoleWriter;
                let _ = w.write_str(prefix);
                let _ = w.write_str(msg);
                let _ = w.write_str("\x1b[0m\n");
            }
        }
    }
}

// ----------------------------------------------------------------------------
// helpers
// ----------------------------------------------------------------------------

/// Render a `u64` into a fixed-size heapless::String (no allocator).
fn format_u64(mut v: u64) -> heapless::String<20> {
    let mut s: heapless::String<20> = heapless::String::new();
    if v == 0 {
        let _ = s.push('0');
        return s;
    }
    let mut tmp = [0u8; 20];
    let mut n = 0;
    while v > 0 && n < 20 {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        let _ = s.push(tmp[n] as char);
    }
    s
}

// ----------------------------------------------------------------------------
// Panic-time dump
// ----------------------------------------------------------------------------

/// Best-effort: flush the ring buffer to console unconditionally. Used by
/// the panic handler so the user sees the kernel log even if the boot
/// phase was past the threshold.
pub unsafe fn panic_dump() {
    let buf = &LOGBUF;
    let head = buf.head;
    let mut i = 0;
    while i < REC_SLOTS {
        let idx = (head + i) % REC_SLOTS;
        let rec = &buf.slots[idx];
        if rec.seq == 0 {
            i += 1;
            continue;
        }
        let mut w = ConsoleWriter;
        let _ = write!(
            w,
            "[seq={} lv={} tgt={:?}] {}\n",
            rec.seq, rec.level, &rec.target[..rec.target_len as usize],
            core::str::from_utf8(&rec.msg[..rec.msg_len as usize]).unwrap_or("?")
        );
        i += 1;
    }
}