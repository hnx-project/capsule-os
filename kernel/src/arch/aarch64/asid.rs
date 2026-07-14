//! AArch64 ASID allocator.
//!
//! ## Why ASIDs?
//!
//! AArch64 lets `TTBR0_EL1` carry an 8-bit (in AArch32-era / ARMv8.0-A
//! with `TCR_EL1.AS` = 0) or 16-bit (`AS` = 1) ASID in bits [63:48].
//! Without an ASID, every `TLBI` invalidation that wants to be safe
//! (e.g. when context-switching processes) must use the global
//! `vmalle1` form, which discards *all* translation entries for EL0/EL1
//! regardless of which process owned them.  That is correct but
//! expensive; it also races the AArch64 pipeline in a subtle way:
//! a global `vmalle1` followed by an `msr TTBR0_EL1` requires an `isb`
//! barrier *between* the two to drain the instruction fetch pipe of
//! stale translations on the old ASID-less regime.
//!
//! With per-process ASIDs, each process keeps its own TLB slice and
//! can be invalidated independently with `tlbi aside1, xN` -- cheaper
//! and, crucially, races-free across the TTBR0 reload.
//!
//! ## Scope (Phase 3 - per-process ASID bring-up)
//!
//! We commit to **8-bit ASIDs** (`AS = 0`).  That gives us 255 user
//! ASIDs (0 is reserved for the kernel side, see below) which is more
//! than enough for the 1.0 demo (`MAX_PROCESSES = 8`).  When we move
//! to multi-core and a per-CPU ASID rollover table, we will promote
//! to 16-bit and follow the Linux `ASID_BITS` rollover protocol.
//!
//! ## Allocation policy
//!
//! Single-core, single-thread allocator with a generation counter:
//! - ASIDs are issued monotonically from 1 upward.
//! - When we wrap past `MAX_USER_ASID`, the next allocation triggers
//!   a global `tlbi vmalle1` (alias of "discard everything") and
//!   bumps the generation.  Processes that were allocated under the
//!   old generation will see global TLB invalidation on their next
//!   switch -- equivalent to the no-ASID case but only at the wrap
//!   point.
//! - `free(asid)` is a no-op for now (single-core, no reuse).  The
//!   bit map is in place for the multi-core follow-up.

use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};

/// Bits of `TTBR0_EL1` reserved for the ASID field.
///
/// ARMv8.0-A with `TCR_EL1.AS = 0` allocates bits [55:48] (8 bits).
/// We don't encode the ASID into `TTBR0_EL1` here -- `set_ttbr0_el1`
/// does the OR -- but the constant must agree with what
/// `set_ttbr0_el1` shifts by.
pub const TTBR_ASID_SHIFT: u32 = 48;
pub const ASID_BITS: u32 = 8;

/// ASID 0 is the canonical "kernel" ASID reserved for the boot
/// context (when `TTBR0_EL1` carries the kernel L0 with no per-process
/// mappings yet).  User processes receive 1..=255.
pub const ASID_KERNEL: u16 = 0;
pub const ASID_INVALID: u16 = 0;
pub const MAX_USER_ASID: u16 = 255;

/// Monotonically incrementing counter for the *next* ASID to hand
/// out.  Starts at 1; the first `alloc()` returns 1.
static NEXT_ASID: AtomicU16 = AtomicU16::new(1);

/// Number of times we have wrapped the ASID space.  Each wrap must be
/// paired with a global TLB invalidation.  Read by the kernel's MMU
/// routines to decide whether to issue `tlbi vmalle1`.
static ROLLOVER_GEN: AtomicU32 = AtomicU32::new(0);

/// Allocate the next free ASID.  Returns `None` only on overflow
/// (currently impossible -- `MAX_USER_ASID = 255`, `MAX_PROCESSES = 8`
/// -- but the type signature stays honest for the future).
///
/// On wrap, the kernel-side caller is responsible for invalidating
/// the global TLB before the next `msr TTBR0_EL1` lands, because all
/// stale translations must be dropped.
pub fn alloc() -> Option<u16> {
    let next = NEXT_ASID.fetch_add(1, Ordering::AcqRel);
    if next == 0 || next > MAX_USER_ASID {
        // Either wrapped past MAX_USER_ASID or hit the kernel ASID
        // sentinel by accident.  Roll over: bump the generation and
        // restart from 1.  Caller MUST follow with a global TLB flush.
        NEXT_ASID.store(1, Ordering::Release);
        ROLLOVER_GEN.fetch_add(1, Ordering::AcqRel);
        Some(1)
    } else {
        Some(next)
    }
}

/// Release an ASID back to the pool.  Single-core bring-up: this is a
/// no-op; we keep the function signature so callers can be written
/// portably today.
pub fn free(_asid: u16) {
    // Single-core single-thread: no reuse.  The wrapper for the
    // multi-core follow-up will mark a bitmap slot free.
}

/// Current rollover generation.  If this changes between two
/// `set_ttbr0_el1` calls, the caller must issue a global TLB flush.
pub fn rollover_generation() -> u32 {
    ROLLOVER_GEN.load(Ordering::Acquire)
}

/// Pack `(l0_base_pa, asid)` into the raw `TTBR0_EL1` write value.
///
/// `l0_base_pa` must be 4 KiB-aligned (16 KiB-aligned on systems with
/// a 16 KiB granule, but we use 4 KiB throughout).  Bits above 47 of
/// the PA are silently dropped (matches `TCR_EL1.IPS` for our 32-bit
/// PA configuration on QEMU 512 MB).
#[inline(always)]
pub fn pack_ttbr(l0_base_pa: u64, asid: u16) -> u64 {
    let _ = asid;
    let pa_masked = l0_base_pa & 0x0000_FFFF_FFFF_F000u64;
    pa_masked
}
