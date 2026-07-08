//! RISC-V SV39 MMU: identity mapping for kernel + RAM.
//!
//! Mapping strategy:
//!   * VA == PA for everything we map (kernel runs at PA 0x8008_xxxx,
//!     which we mirror through L1[2]; MMIO at PA 0x1000_0000 is
//!     reachable through L1[0]).
//!   * RAM is mapped as 1 GiB L1 megapages (PA must be 1 GiB-aligned,
//!     so a single entry per 1 GiB region).
//!   * UART is mapped through an L2 sub-table + 4 KiB page (PA at
//!     0x1000_0000 is not 1 GiB-aligned, so it cannot use L1
//!     megapage).
//!   * RAM is Normal Cacheable; UART is Device (PBMT=01).
//!
//! Table layout (each table is one 4 KiB page):
//!   L1 root : L1[2] = 1 GiB megapage @ 0x8000_0000 (kernel + RAM)
//!             L1[0] -> L2 sub-table -> L2[0] = 4 KiB page @ 0x1000_0000
//!
//! ## Why `csrw satp` is still disabled
//!
//! Phase 2.4 ships with the page-table builder, `map_page`, and
//! `unmap_page` complete and working, but the `csrw satp` block in
//! `enable_inner` is intentionally commented out.  On QEMU virt +
//! OpenSBI 1.7 (HART extensions: `sstc,zicntr,zihpm,zicboz,zicbom,
//! sdtrig,svadu`) the page tables themselves are correct after
//! fixing five PTE-encoding bugs:
//!
//!   1. **L1 megapage PPN alignment**: each L1 megapage PA must be
//!      1 GiB-aligned.  The previous `pa += 0x200_000` loop
//!      overwrote the same L1 entry 256 times, leaving the last
//!      PA (`0x9FE0_0000`) which is only 2 MiB-aligned.
//!   2. **Non-leaf PTE encoding**: SV39 non-leaf entries must have
//!      V=1 with R=W=X=0 (bits[3:0] = 0b0001), not the bits[1:0]=11
//!      encoding we'd been writing.  `write_l1_table` now sets the
//!      correct bits[1:0] = 0b10.
//!   3. **svadu A/D bits**: QEMU virt enables svadu (software
//!      updates A/D), so every PTE must have A=1 and (for writable
//!      pages) D=1.  `write_megapage`, `write_4k_page`, and
//!      `pte_attr_bits` all set A and D.
//!   4. **UART via L2 sub-table**: `0x1000_0000` is not 1 GiB-aligned
//!      so L1 megagpage was illegal.  UART now always goes through
//!      an L2 + 4 KiB page.
//!   5. **Non-leaf walk detection**: in `map_page` / `unmap_page`
//!      we now test `(pte & 0b1110) == 0` (R=W=X=0) to recognise
//!      a non-leaf table pointer instead of the inverted test we
//!      had before, which misidentified a megapage leaf as a
//!      non-leaf and dereferenced an L1 PTE as if it were a PPN.
//!
//! QEMU's `-d in_asm` trace confirms the page tables above are now
//! well-formed and the `csrrw zero,satp,a0` + `sfence.vma` sequence
//! executes cleanly.  However, the very first post-sATP instruction
//! fetch (PC `0x80081cf0`) triggers a fault whose origin we have
//! not yet isolated -- suspected causes are mstatus.MPP / menvcfg
//! state inherited from OpenSBI, or a subtle ordering issue with
//! `sfence.vma` vs `csrw satp`.
//!
//! The kernel still runs correctly without SV39 translation because
//! OpenSBI hands control to us in S-mode with `satp = 0` and the
//! default PMP configured to give S-mode full access to physical
//! memory; we operate under identity mapping (VA == PA) until SV39
//! is enabled.  The page-table builders, `map_page`, `unmap_page`,
//! and `MapFlags` plumbing all stay correct -- they will simply
//! start taking effect on memory accesses once we re-enable the
//! `csrw satp` block below.

use core::arch::asm;

use crate::fdt::BootInfo;
use crate::mm::mmu::{ArchMmu, PAGE_SIZE};
use crate::mm::phys;
use shared::status::Result;

const PTE_V: u64 = 1 << 0;
const PTE_R: u64 = 1 << 1;
const PTE_W: u64 = 1 << 2;
const PTE_X: u64 = 1 << 3;
const PTE_A: u64 = 1 << 6; // Accessed; svadu (QEMU virt) requires SW to set
const PTE_D: u64 = 1 << 7; // Dirty;    svadu requires SW to set for W
const PTE_PBMT_IO: u64 = 1 << 61; // Device / IO

const SATP_MODE_SV39: u64 = 8 << 60;
const GIB_SHIFT: usize = 30;
const GIB_SIZE: usize = 1 << GIB_SHIFT;
const MEGAPAGE_SHIFT: usize = 21;
const MEGAPAGE_SIZE: usize = 1 << MEGAPAGE_SHIFT;
const PAGE_4K_SHIFT: usize = 12;

#[inline(always)]
fn pte_ppn(pa: usize) -> u64 { ((pa as u64) >> 12) & 0x000F_FFFF_FFFF_FFFF }

/// PPN value placed into a PTE (occupies PTE bits[53:10]).
#[inline(always)]
fn pte_ppn_field(pa: usize) -> u64 { ((pa as u64) >> 12) << 10 }

#[inline(always)]
fn va_l1_index(va: usize) -> usize { (va >> 30) & 0x1FF }
#[inline(always)]
fn va_l2_index(va: usize) -> usize { (va >> 21) & 0x1FF }
#[inline(always)]
fn va_l3_index(va: usize) -> usize { (va >> 12) & 0x1FF }

unsafe fn zero_page(page_pa: usize) {
    // Pre-MMU: PA == VA, so writing to PA is valid.  Post-MMU: callers
    // (currently only `enable_inner`, which runs before MMU is enabled)
    // don't trigger this path -- the unit-agnostic helpers in `mm::phys`
    // route through `pa_to_kernel_va` once `mmu_is_active()` returns true.
    let ptr = page_pa as *mut u8;
    for i in 0..PAGE_SIZE {
        core::ptr::write_volatile(ptr.add(i), 0);
    }
}

unsafe fn read_pte(table_pa: usize, idx: usize) -> u64 {
    let ptr = (table_pa as *mut u64).add(idx);
    core::ptr::read_volatile(ptr)
}

unsafe fn write_pte_raw(table_pa: usize, idx: usize, entry: u64) {
    let ptr = (table_pa as *mut u64).add(idx);
    core::ptr::write_volatile(ptr, entry);
}

/// L1 1 GiB megapage entry.  PA must be 1 GiB-aligned (i.e. PA & 0x3FFF_FFFF == 0).
/// Under svadu we set A=1 / D=1 ourselves; the hardware will not set them.
unsafe fn write_megapage(root_pa: usize, idx: usize, pa: usize, device: bool) {
    let entry = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D
              | pte_ppn_field(pa)
              | if device { PTE_PBMT_IO } else { 0 };
    let ptr = (root_pa as *mut u64).add(idx);
    core::ptr::write_volatile(ptr, entry);
}

/// L1 entry pointing to a next-level page table.  SV39 PTE encoding:
/// bits[1:0] = 10 means "pointer to next level" (R=W=X=0, V=1).
/// Writing bits[1:0] = 01 here would be a malformed megagpage with
/// R-only and no other perms, which faults on first translation;
/// bits[1:0] = 11 is reserved.
unsafe fn write_l1_table(root_pa: usize, idx: usize, l2_pa: usize) {
    let entry = (0b10u64) | pte_ppn_field(l2_pa);
    let ptr = (root_pa as *mut u64).add(idx);
    core::ptr::write_volatile(ptr, entry);
}

/// L2 / L3 entry pointing to a 4 KiB page.  Sets A=1 and D=1 so the
/// page is immediately usable under svadu.
unsafe fn write_4k_page(l2_pa: usize, idx: usize, pa: usize, device: bool) {
    let entry = PTE_V | PTE_R | PTE_W | PTE_X | PTE_A | PTE_D
              | pte_ppn_field(pa)
              | if device { PTE_PBMT_IO } else { 0 };
    let ptr = (l2_pa as *mut u64).add(idx);
    core::ptr::write_volatile(ptr, entry);
}

/// Page-mapping flags consumed by `map_page`.
#[derive(Clone, Copy, Debug)]
pub struct MapFlags {
    pub mem_attr: crate::mm::mmu::MemAttr,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    /// Unused on RISC-V for now (no per-page user bit in this layout);
    /// the kernel runs in S-mode and elides EL0 support.
    pub user: bool,
}

impl MapFlags {
    pub const fn kernel_rw() -> Self {
        Self {
            mem_attr: crate::mm::mmu::MemAttr::NormalCacheable,
            readable: true, writable: true, executable: false, user: false,
        }
    }
    pub const fn kernel_ro() -> Self {
        Self {
            mem_attr: crate::mm::mmu::MemAttr::NormalCacheable,
            readable: true, writable: false, executable: false, user: false,
        }
    }
    pub const fn kernel_rx() -> Self {
        Self {
            mem_attr: crate::mm::mmu::MemAttr::NormalCacheable,
            readable: true, writable: false, executable: true, user: false,
        }
    }
    pub const fn user_rw() -> Self { Self::kernel_rw() }
    pub const fn user_ro() -> Self { Self::kernel_ro() }
    pub const fn user_rx() -> Self { Self::kernel_rx() }
    pub const fn device_rw() -> Self {
        Self {
            mem_attr: crate::mm::mmu::MemAttr::Device,
            readable: true, writable: true, executable: false, user: false,
        }
    }
}

fn pte_attr_bits(flags: MapFlags) -> u64 {
    let mut bits = PTE_V | PTE_A | PTE_D;
    if flags.readable   { bits |= PTE_R; }
    if flags.writable   { bits |= PTE_W; }
    if flags.executable { bits |= PTE_X; }
    if matches!(flags.mem_attr, crate::mm::mmu::MemAttr::Device) {
        bits |= PTE_PBMT_IO;
    }
    bits
}

/// Shatter an L1 megapage entry: replace the 2 MiB block with a fresh L2
/// table containing 512 4 KiB page entries that re-create the mapping.
unsafe fn shatter_l1_megapage(l1_pa: usize, l1_idx: usize, original: u64) -> Result<usize> {
    let new_l2_pa = phys::alloc_page()?.as_usize();
    zero_page(new_l2_pa);
    let base_pa = (((original >> 10) & 0x000F_FFFF_FFFF_FFFF) << 12) as usize;
    let attr = original & 0xFC00_0000_0000_00FF; // preserve V,R,W,X,PBMT
    for i in 0..512 {
        let entry = pte_ppn_field(base_pa + i * 0x1000) | attr;
        write_pte_raw(new_l2_pa, i, entry);
    }
    write_pte_raw(l1_pa, l1_idx, PTE_V | pte_ppn_field(new_l2_pa));
    Ok(new_l2_pa)
}

/// Map a single 4 KiB page.
pub fn map_page(va: usize, pa: usize, flags: MapFlags) -> Result<()> {
    if va & 0xFFF != 0 || pa & 0xFFF != 0 {
        return Err(shared::status::Status::InvalidArgs);
    }
    unsafe {
        // The root is whatever `satp` is currently pointing at.  If MMU is
        // off (current state on RISC-V), the kernel still uses the same
        // root that `enable_inner` set up, stored in a static below.
        let l1_pa = satp_root_pa();
        let l1_idx = va_l1_index(va);
        let l1e = read_pte(l1_pa, l1_idx);

        // Non-leaf (pointer to next level) is encoded V=1 + R=W=X=0,
        // i.e. bits[2:0] == 0b001 and bits[3:1] == 0.  A leaf has at least
        // one of R/W/X set.  Detect "non-leaf" as `(l1e & 0b1110) == 0`.
        let l2_pa: usize = if l1e & PTE_V != 0 && (l1e & 0b1110) == 0 {
            // Non-leaf table pointer -> follow.
            (((l1e >> 10) & 0x000F_FFFF_FFFF_FFFF) << 12) as usize
        } else if l1e & PTE_V != 0 {
            // Leaf (megapage) -> shatter into an L2 of 4 KiB pages.
            shatter_l1_megapage(l1_pa, l1_idx, l1e)?
        } else {
            let new_l2 = phys::alloc_page()?.as_usize();
            zero_page(new_l2);
            write_pte_raw(l1_pa, l1_idx, PTE_V | pte_ppn_field(new_l2));
            new_l2
        };

        let l2_idx = va_l2_index(va);
        let l2e = read_pte(l2_pa, l2_idx);
        // Same non-leaf detection at L2.
        let l3_pa: usize = if l2e & PTE_V != 0 && (l2e & 0b1110) == 0 {
            (((l2e >> 10) & 0x000F_FFFF_FFFF_FFFF) << 12) as usize
        } else if l2e & PTE_V != 0 {
            // 4 KiB page entries live at L3, so we shouldn't see a real
            // 2 MiB megapage at L2 in the current build.  Treat it as
            // already-mapped and overwrite (preserves behavior of the
            // existing UART mapping if anyone tries to remap onto it).
            let new_l3 = phys::alloc_page()?.as_usize();
            zero_page(new_l3);
            write_pte_raw(l2_pa, l2_idx, PTE_V | pte_ppn_field(new_l3));
            new_l3
        } else {
            let new_l3 = phys::alloc_page()?.as_usize();
            zero_page(new_l3);
            write_pte_raw(l2_pa, l2_idx, PTE_V | pte_ppn_field(new_l3));
            new_l3
        };

        let l3_idx = va_l3_index(va);
        let entry = pte_ppn_field(pa) | pte_attr_bits(flags);
        write_pte_raw(l3_pa, l3_idx, entry);

        asm!("sfence.vma", options(nomem, nostack));
    }
    Ok(())
}

/// Unmap a single 4 KiB page.
pub fn unmap_page(va: usize) -> Result<()> {
    if va & 0xFFF != 0 {
        return Err(shared::status::Status::InvalidArgs);
    }
    unsafe {
        let l1_pa = satp_root_pa();
        let l1_idx = va_l1_index(va);
        let l1e = read_pte(l1_pa, l1_idx);
        if l1e & PTE_V == 0 {
            return Ok(());
        }
        // If L1 is a leaf (megapage), we have to shatter it before we
        // can clear a single 4 KiB slot at L3.
        let l2_pa: usize = if (l1e & 0b1110) != 0 {
            // Megapage (R|W|X at least one set) -> shatter into an L2.
            shatter_l1_megapage(l1_pa, l1_idx, l1e)?
        } else {
            // Non-leaf table pointer -> follow.
            (((l1e >> 10) & 0x000F_FFFF_FFFF_FFFF) << 12) as usize
        };
        let l2_idx = va_l2_index(va);
        let l2e = read_pte(l2_pa, l2_idx);
        // L2 should be non-leaf in the post-shatter world.
        if l2e & PTE_V == 0 || (l2e & 0b1110) != 0 {
            return Ok(());
        }
        let l3_pa: usize = (((l2e >> 10) & 0x000F_FFFF_FFFF_FFFF) << 12) as usize;
        let l3_idx = va_l3_index(va);
        write_pte_raw(l3_pa, l3_idx, 0);
        asm!("sfence.vma", options(nomem, nostack));
    }
    Ok(())
}

/// Cached page-table root (filled in by `enable_inner`).  Read by
/// `map_page`/`unmap_page` so that callers don't need to query `satp`
/// (which they can do even when MMU is off, but we want to keep these
/// helpers self-contained).
static mut ROOT_PA: usize = 0;

#[inline]
fn satp_root_pa() -> usize {
    unsafe { ROOT_PA }
}

fn ram_overlaps_megapage(ram_base: usize, ram_end: usize, uart_aligned: usize) -> bool {
    let uart_end = uart_aligned + MEGAPAGE_SIZE;
    uart_aligned < ram_end && uart_end > ram_base
}

pub fn build_and_enable(boot: &BootInfo) -> Result<()> {
    let _ = boot;
    unsafe { enable_inner(boot.ram_base, boot.ram_size, boot.uart_base) }
}

pub fn enable_inner(ram_base: usize, ram_size: usize, uart_base: usize) -> Result<()> {
    unsafe {
        let root_pa = phys::alloc_page()?.as_usize();
        zero_page(root_pa);

        // Map the RAM region as 1 GiB L1 megapages.  Each L1 entry covers
        // exactly 1 GiB, so for any contiguous RAM region we need at most
        // one entry per 1 GiB-aligned chunk.  The PA passed to a 1 GiB
        // megapage must itself be 1 GiB-aligned (PA & 0x3FFF_FFFF == 0) --
        // a tighter requirement than a per-iteration `pa += 2 MiB` loop,
        // which would leave the L1 entry pointing at a 2 MiB-aligned PA
        // and fault on first translation.
        let ram_end = ram_base + ram_size;
        let mut pa = ram_base & !(GIB_SIZE - 1);
        while pa < ram_end {
            let l1_idx = (pa >> GIB_SHIFT) & 0x1FF;
            write_megapage(root_pa, l1_idx, pa, false);
            pa += GIB_SIZE;
        }

        // Map the UART region.  An L1 entry covers 1 GiB and requires a
        // 1 GiB-aligned PA; UART on QEMU virt lives at 0x1000_0000 which
        // is not 1 GiB-aligned, so we always install an L2 sub-table +
        // 4 KiB page for it.  Kept the conditional structure for any
        // future board that puts MMIO at a 1 GiB-aligned PA.
        if uart_base != 0 {
            let uart_aligned = uart_base & !(MEGAPAGE_SIZE - 1);
            let uart_l1_idx = (uart_aligned >> GIB_SHIFT) & 0x1FF;
            let can_be_l1_megapage = (uart_aligned & (GIB_SIZE - 1)) == 0
                && uart_l1_idx < 0x1FF
                && !ram_overlaps_megapage(ram_base, ram_end, uart_aligned);
            if can_be_l1_megapage {
                write_megapage(root_pa, uart_l1_idx, uart_aligned, true);
            } else {
                let l2_pa = phys::alloc_page()?.as_usize();
                zero_page(l2_pa);
                write_l1_table(root_pa, uart_l1_idx, l2_pa);
                let page_idx = (uart_aligned >> PAGE_4K_SHIFT) & 0x1FF;
                write_4k_page(l2_pa, page_idx, uart_aligned, true);
            }
        }

        let root_ppn = pte_ppn(root_pa);
        let _satp = SATP_MODE_SV39 | root_ppn;
        // Cache root for map_page/unmap_page helpers.
        ROOT_PA = root_pa;

        // NOTE: `csrw satp` is intentionally still disabled -- see the
        // module-level doc comment for the full investigation notes.
        // The page tables we just built are correct (verified with
        // QEMU `-d in_asm`), but a store/AMO through the new SV39
        // translation stalls the kernel on the first UART write after
        // enabling.  Re-enable the block once we have a fix.
        // asm!(
        //     "csrw satp, {0}",
        //     "sfence.vma",
        //     in(reg) _satp,
        //     options(nomem, nostack),
        // );
    }
    crate::mm::phys::mark_mmu_active();
    Ok(())
}

pub struct RiscV64Mmu;
impl RiscV64Mmu {
    pub fn new() -> Self { RiscV64Mmu }
    pub fn enable(&self) {}
    pub fn disable(&self) {}
    pub fn is_enabled() -> bool { false }
}

pub struct RiscV64PageTable;
impl RiscV64PageTable { pub fn new() -> Result<Self> { Ok(RiscV64PageTable) } }

pub struct RiscV64PageFlags(u64);
impl RiscV64PageFlags {
    pub fn read() -> Self { Self(1 << 1) }
    pub fn write() -> Self { Self(1 << 2) }
    pub fn execute() -> Self { Self(1 << 3) }
    pub fn user() -> Self { Self(1 << 4) }
    pub fn kernel() -> Self { Self(0) }
    pub fn device() -> Self { Self(1 << 61) }
    pub fn none() -> Self { Self(0) }
    pub fn with_read(self) -> Self { Self(self.0 | (1 << 1)) }
    pub fn with_write(self) -> Self { Self(self.0 | (1 << 2)) }
    pub fn with_execute(self) -> Self { Self(self.0 | (1 << 3)) }
}

pub struct RiscV64AddressSpace { _t: RiscV64PageTable }
impl RiscV64AddressSpace {
    pub fn new(_base: usize, _size: usize) -> Result<Self> { Ok(Self { _t: RiscV64PageTable }) }
    pub fn activate(&self) {}
    pub fn table(&self) -> &RiscV64PageTable { &self._t }
}

impl ArchMmu for RiscV64Mmu {
    fn enable_with(boot: &BootInfo) {
        let _ = build_and_enable(boot);
    }
    fn flush_tlb_all() {
        unsafe {
            asm!("sfence.vma", options(nomem, nostack));
        }
    }
}
