# L3 Page Table Corruption Investigation

## Goal
- Debug persistent corruption of PID 1's L3 page table entries when PID 2 (devmgr) runs

## Constraints & Preferences
- AArch64 QEMU `cortex-a72` with semihosting
- Bump allocator with pre-check tag guard already added

## Instrumentation Added

### Dynamic WATCH_PA (全局动态监视地址)
- Added `WATCH_PA: AtomicUsize` in `kernel/src/mm/phys.rs` — set during PID 1's FINAL-CANARY to the actual L3 page PA for that boot
- All WATCH statements that formerly used hardcoded addresses now also compare against this value, making instrumentation reliable across non-deterministic cursor progress
- Files modified: `phys.rs`, `page_table.rs`, `mmu.rs` (aarch64), `vmo.rs`, `loader.rs`, `scheduler.rs`, `process.rs`

### Live Page-Table Page Tracker
- Added `LIVE_PT_PAGES: [AtomicUsize; 64]` in `phys.rs` to track all currently-in-use page-table PAs
- `register_pt_page(pa)` called from `PageTableTree::track()`
- `unregister_pt_page(pa)` called from `phys::free_page` when tag == PageTable
- `alloc_page` panics if the candidate PA is still in `LIVE_PT_PAGES`
- Files modified: `phys.rs`, `page_table.rs`

### pa_to_kernel_va WATCH Hook
- Added WATCH check inside `pa_to_kernel_va` itself — catches EVERY code path that computes the KVA of the watched page
- Files modified: `kernel/src/mm/mmu.rs`

### safe_copy_to_user / safe_copy_from_user WATCH
- Added WATCH in both functions to detect writes/reads to the watched L3 page through user-copy paths
- Files modified: `kernel/src/syscall/handlers/ipc.rs`

## Key Findings

### Corruption Pattern
- PID 1's L3 page PA shifts between runs (e.g., `0x40258000`, `0x4025d000`, `0x4025e000`) due to non-deterministic bump allocator cursor progress
- At launch (FINAL-CANARY): `L3[0..3]` = 4 valid PTEs (e.g., `0x60000040259743` etc.), `L3[4..7]` = 0
- After PID 2's launch (loader.rs CANARY detects corruption):
  - `L3[0]` = `0x400da598` (NOT zero — looks like kernel metadata)
  - `L3[1]` = `0xa4025ee38` (contains embedded L3 page PA reference)
  - `L3[2]` = `0xffff80004025e068` (the KVA of the L3 page itself!)
  - `L3[3..7]` = 0 (later scheduler PRE-ERET shows full zeroing)

### What Was Ruled Out

| Path | Result |
|------|--------|
| `write_pte(table_pa, idx, 0)` | WATCH did NOT fire for L3 page PA |
| `alloc_page` (reallocation) | Tag guard + LIVE_PT tracker did NOT fire |
| `free_page` | WATCH did NOT fire for L3 page PA |
| `Vmo::write` / `Vmo::commit_page` | WATCH did NOT fire for L3 page PA |
| `safe_copy_to_user` / `safe_copy_from_user` | WATCH did NOT fire for L3 page PA |
| `pa_to_kernel_va` (after FINAL-CANARY) | Only 1 extra call between FINAL-CANARY and CANARY detection |

### Critical Open Question
**`pa_to_kernel_va(0x4025e000)` is called ONCE between PID 2's FINAL-CANARY and the corruption detection CANARY.** This call is NOT from:
- FINAL-CANARY dump (which reads the new process's L3 page, not PID 1's)
- loader.rs CANARY (which comes after)
- safe_copy_to_user/free_page/alloc_page/write_pte

This single mysterious call is the write that corrupts L3[0] and L3[1]. The subsequent zeroing of L3[2..7] happens later (during context switch to PID 1).

### Most Likely Theory
The corruption values contain KVAs that reference the L3 page itself (`0xffff80004025e068`). This suggests the data overwriting the L3 page is from a kernel data structure that stores KVA pointers. Candidates:
- `VmarStorage` (VMAR metadata page)
- `VmoHeader` / VMO page slots
- Handle table entries

The progressive nature (first L3[0..2] overwritten with metadata, then all entries zeroed) suggests a two-stage process: first a data structure write partially overwrites the page, then something (possibly the scheduler context-switch path) zeros the rest.

## Next Steps
1. Identify the mysterious `pa_to_kernel_va` call at step 7 (between PID 2's FINAL-CANARY and the CANARY detection)
2. Add source-location tracking to `pa_to_kernel_va` (e.g., with a file/line parameter or unique ID) to identify the caller
3. Consider adding `-d trace:int` or other QEMU logging to trace memory access patterns
4. If the corruption is from a data structure overflow, fix the overflow (buffer overflow in VMO/VMAR metadata)
5. If from a use-after-free reallocation, fix the lifecycle issue
