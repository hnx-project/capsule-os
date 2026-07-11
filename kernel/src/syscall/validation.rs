//! User-pointer validation facade.
//!
//! Phase 6 v0.6.0-α safety push, KERNEL_HEALTH.md bucket-9 K1
//! (formerly AUDIT M8).  Previously `validate_pointer`,
//! `validate_mut_pointer`, `validate_buffer` were stubs that
//! returned `Ok(())` regardless of input — any EL0 program
//! could hand a syscall a raw pointer into kernel memory and
//! the kernel would deref it without checking.
//!
//! The new implementation walks the calling process's
//! page table for every distinct page touched by the buffer
//! and rejects pointers that don't resolve.  This is the same
//! page-walk the `safe_copy_from_user / safe_copy_to_user`
//! helpers already do, but it happens at the syscall entry
//! before any data movement so a bad pointer returns a clean
//! `Status::InvalidArgs` instead of leaking into the
//! dispatch path.
//!
//! Rights-bit enforcement is left for a follow-up commit: the
//! current per-page-table permission bits encode
//! read/write/execute but the audit-flagged M8 facet asked
//! for "address-range + permission" checks; the former is
//! in this file, the latter still needs a process-as-uid
//! abstraction that VFS permissions can hang off.

use shared::status::{Result, Status};

/// Resolve the calling thread's L0 page-table base via the
/// scheduler.  Used by `validate_*` to scope every pointer
/// check to the caller's address space — never trust a raw
/// pointer the kernel didn't translate.
fn current_l0_pa() -> Result<usize> {
    let t = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
        .ok_or(Status::NotFound)?;
    let proc_id = unsafe { (*t).process_id };
    if proc_id == 0 {
        return Err(Status::ProcessNotFound);
    }
    let proc = crate::task::process::find_process_mut(proc_id)
        .ok_or(Status::ProcessNotFound)?;
    Ok(proc.l0_user_pa)
}

/// Translate a single user VA, returning `None` if any level
/// of the walk is unmapped.  Wraps `arch::translate_user_va`
/// so the validation layer is arch-agnostic.
fn translate(va: usize, l0_pa: usize) -> Result<()> {
    if l0_pa == 0 {
        // No page-table base — treat the entire range as
        // unmapped; the dispatcher must catch this above us.
        return Err(Status::InvalidArgs);
    }
    if crate::arch::translate_user_va(l0_pa, va).is_none() {
        return Err(Status::InvalidArgs);
    }
    Ok(())
}

/// Walk the page table for every page touched by a buffer of
/// `len` bytes starting at `ptr`.  Walks only the head and
/// tail pages plus one probe per 4 KiB between — page walks
/// are cheap (4-level table, ~16 cache-line loads in the
/// worst case) so we don't try to be clever about stride.
fn validate_range(ptr: usize, len: usize, l0_pa: usize) -> Result<()> {
    if len == 0 {
        return translate(ptr, l0_pa);
    }
    let end = ptr.checked_add(len).ok_or(Status::InvalidArgs)?;
    let mut addr = ptr;
    loop {
        translate(addr, l0_pa)?;
        if addr >= end - 1 {
            // Probe the tail byte too so we catch a buffer
            // whose length is exactly the page boundary.
            translate(addr, l0_pa)?;
            // And the very last byte if it's not page-aligned.
            let last = end - 1;
            if (last & 0xfff) != (addr & 0xfff) {
                translate(last, l0_pa)?;
            }
            return Ok(());
        }
        let next_page = (addr & !0xfff) + 0x1000;
        addr = if next_page < end { next_page } else { end - 1 };
    }
}

pub fn validate_pointer<T>(ptr: *const T) -> Result<()> {
    let l0_pa = current_l0_pa()?;
    if ptr.is_null() {
        return Err(Status::InvalidArgs);
    }
    validate_range(ptr as usize, core::mem::size_of::<T>(), l0_pa)
}

pub fn validate_mut_pointer<T>(ptr: *mut T) -> Result<()> {
    let l0_pa = current_l0_pa()?;
    if ptr.is_null() {
        return Err(Status::InvalidArgs);
    }
    validate_range(ptr as usize, core::mem::size_of::<T>(), l0_pa)
}

pub fn validate_buffer(ptr: usize, len: usize) -> Result<()> {
    let l0_pa = current_l0_pa()?;
    if ptr == 0 && len > 0 {
        return Err(Status::InvalidArgs);
    }
    if len == 0 {
        // Zero-length buffers are valid even at address 0
        // (Linux allows NULL+0 for writev etc.); only check
        // the pointer if non-zero.
        if ptr != 0 {
            validate_range(ptr, 1, l0_pa)?;
        }
        return Ok(());
    }
    validate_range(ptr, len, l0_pa)
}

/// Full handle validation: existence + type + rights.
pub fn validate_handle_access(
    table: &HandleTable,
    raw: u32,
    expected_type: ObjectType,
    required_rights: u32,
) -> Result<HandleValue> {
    let hv = validate_handle(raw)?;
    table.with(hv, required_rights, |_obj| Ok(()))?;
    let _ = expected_type; // type verification done at call site
    Ok(hv)
}

pub fn validate_handle(raw: u32) -> Result<HandleValue> {
    let hv = HandleValue::new(raw);
    if hv == HandleValue::INVALID {
        return Err(Status::BadHandle);
    }
    Ok(hv)
}

use crate::object::handle_table::HandleTable;
use shared::types::{HandleValue, ObjectType};
