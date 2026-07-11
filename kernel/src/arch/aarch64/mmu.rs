//! AArch64 MMU: 4-level page tables, 4 KiB granule, 48-bit VA.
//!
//! Mapping strategy (per Phase 2.3 decision):
//!   * High-half kernel:   VA = PA + 0xFFFF_8000_0000_0000
//!   * Identity mapping:   kept enabled at all times (covers kernel + UART)
//!
//! The high-half offset is chosen so the kernel's virtual address lands in
//! a 1 GiB L1 slot that does not collide with the identity-mapped low half.
//! With `0xFFFF_8000_0000_0000`, kernel PA 0x4008_0000 -> VA
//! 0xFFFF_8000_4008_0000, whose L0 index is 0x1FF and L1 index is 0x02.
//!
//! Table layout (each page = 512 entries of u64 = 4 KiB):
//!   L0         : L0[0]   -> L1_ID  (identity)
//!                L0[0x1FF] -> L1_HIGH (high-half kernel)
//!   L1_ID      : L1_ID[0] = Block 1 GiB @ 0x0000_0000 (Device, UART 0x0900_0000)
//!                L1_ID[1] = Block 1 GiB @ 0x4000_0000 (Normal, kernel + RAM)
//!   L1_HIGH    : L1_HIGH[2] = Block 1 GiB @ 0x4000_0000 (Normal, mirror)

use core::arch::asm;

use crate::fdt::BootInfo;
use crate::mm::mmu::{ArchMmu, KERNEL_OFFSET, MemAttr, PAGE_SIZE, pa_to_kernel_va};
use crate::mm::phys;
use shared::status::Result;

const PTE_VALID: u64 = 1 << 0;
const PTE_TYPE_BLOCK: u64 = 1;       // bits[1:0] = 01
const PTE_TYPE_TABLE: u64 = 3;       // bits[1:0] = 11
const PTE_TYPE_PAGE: u64 = 3;        // bits[1:0] = 11; L3 page entries share encoding with table
const PTE_AF: u64 = 1 << 10;
const PTE_NSH: u64 = 0b00 << 8;      // non-shareable
const PTE_ISH: u64 = 0b11 << 8;      // inner-shareable
const PTE_NORMAL_WB: u64 = (0u64 << 2) | PTE_ISH | PTE_AF; // AttrIdx=0, ISH
const PTE_DEVICE: u64 = (1u64 << 2) | PTE_NSH | PTE_AF;     // AttrIdx=1, nSH
const PTE_USER: u64 = 1 << 6;        // accessible from EL0 (for user pages)
const PTE_AP_USER: u64 = 1 << 6;    // AP[1]=1 (user accessible bit)
const PTE_AP_RO: u64 = 1 << 7;      // AP[2]=1 (read-only bit)
const PTE_XN: u64 = 1 << 54;          // never-execute for now
const PTE_UXN: u64 = 1 << 53;        // unprivileged execute-never

#[inline(always)]
fn va_l0_index(va: usize) -> usize { (va >> 39) & 0x1FF }
#[inline(always)]
fn va_l1_index(va: usize) -> usize { (va >> 30) & 0x1FF }
#[inline(always)]
fn va_l2_index(va: usize) -> usize { (va >> 21) & 0x1FF }
#[inline(always)]
fn va_l3_index(va: usize) -> usize { (va >> 12) & 0x1FF }
#[inline(always)]
fn pa_to_pte_addr(pa: usize) -> u64 { (pa as u64) & 0x0000_FFFF_FFFF_F000 }

unsafe fn zero_page(page_pa: usize) {
    crate::log_info!("MMU", "zero_page start: page_pa={:#x}", page_pa);
    // Pre-MMU: PA is a valid VA (bootloader left us with VA==PA and
    // the kernel still in low memory).  Post-MMU: PA access through
    // `pa_to_kernel_va` works because we built a high-half mirror.
    let ptr = if crate::mm::phys::mmu_is_active() {
        pa_to_kernel_va(page_pa) as *mut u8
    } else {
        page_pa as *mut u8
    };
    for i in 0..PAGE_SIZE {
        core::ptr::write_volatile(ptr.add(i), 0);
    }
    crate::log_info!("MMU", "zero_page end: page_pa={:#x}", page_pa);
}

unsafe fn write_l1_block(table_pa: usize, idx: usize, pa: usize, attr: MemAttr) {
    let entry = pa_to_pte_addr(pa)
        | PTE_VALID
        | PTE_TYPE_BLOCK
        | match attr {
            MemAttr::NormalCacheable => PTE_NORMAL_WB,
            MemAttr::Device => PTE_DEVICE,
        }
        | (0b00u64 << 6) // PTE_AP_RW specifically for privileged EL1-only
        // | PTE_USER       // Make sure it allows user access by default in shared identity region
        | PTE_XN;
    let ptr = (table_pa as *mut u64).add(idx);
    core::ptr::write_volatile(ptr, entry);
}

/// Write a table-descriptor entry into an L0 page-table page.
///
/// **Critical**: when the MMU is active, indexing `l0_pa` as a `*mut u64`
/// would treat the **physical** address as a virtual address and the
/// resulting `dsb/stlr` would land in random DRAM (or page-fault) rather
/// than installing the L0 entry.  Mirror the branch that `read_pte` /
/// `write_pte` already use: when MMU is on, dereference the kernel's
/// direct-map VA (`pa_to_kernel_va(l0_pa)`) instead.  Boot-time identity
/// mapping (MMU off) keeps the raw PA path.
unsafe fn write_l0_table(l0_pa: usize, idx: usize, l1_pa: usize) {
    let entry = pa_to_pte_addr(l1_pa) | PTE_VALID | PTE_TYPE_TABLE;
    let ptr = if crate::mm::phys::mmu_is_active() {
        crate::mm::mmu::pa_to_kernel_va(l0_pa) as *mut u64
    } else {
        l0_pa as *mut u64
    };
    core::ptr::write_volatile(ptr.add(idx), entry);
}

unsafe fn read_pte(table_pa: usize, idx: usize) -> u64 {
    let ptr = if crate::mm::phys::mmu_is_active() {
        pa_to_kernel_va(table_pa) as *mut u64
    } else {
        table_pa as *mut u64
    };
    core::ptr::read_volatile(ptr.add(idx))
}

unsafe fn write_pte(table_pa: usize, idx: usize, entry: u64) {
    let ptr = if crate::mm::phys::mmu_is_active() {
        pa_to_kernel_va(table_pa) as *mut u64
    } else {
        table_pa as *mut u64
    };
    core::ptr::write_volatile(ptr.add(idx), entry);
}

/// Page-mapping flags consumed by `map_page` and `unmap_page`.
#[derive(Clone, Copy, Debug)]
pub struct MapFlags {
    /// Memory type: Normal Cacheable or Device.
    pub mem_attr: MemAttr,
    /// Readable.
    pub readable: bool,
    /// Writable.
    pub writable: bool,
    /// Executable.
    pub executable: bool,
    /// Accessible from EL0 (user) in addition to EL1.
    pub user: bool,
}

impl MapFlags {
    pub const fn kernel_rw() -> Self {
        Self {
            mem_attr: MemAttr::NormalCacheable,
            readable: true,
            writable: true,
            executable: false,
            user: false,
        }
    }
    pub const fn kernel_ro() -> Self {
        Self {
            mem_attr: MemAttr::NormalCacheable,
            readable: true,
            writable: false,
            executable: false,
            user: false,
        }
    }
    pub const fn kernel_rx() -> Self {
        Self {
            mem_attr: MemAttr::NormalCacheable,
            readable: true,
            writable: false,
            executable: true,
            user: false,
        }
    }
    pub const fn user_rw() -> Self {
        Self {
            mem_attr: MemAttr::NormalCacheable,
            readable: true,
            writable: true,
            executable: false,
            user: true,
        }
    }
    pub const fn user_ro() -> Self {
        Self {
            mem_attr: MemAttr::NormalCacheable,
            readable: true,
            writable: false,
            executable: false,
            user: true,
        }
    }
    pub const fn user_rx() -> Self {
        Self {
            mem_attr: MemAttr::NormalCacheable,
            readable: true,
            writable: false,
            executable: true,
            user: true,
        }
    }
    pub const fn device_rw() -> Self {
        Self {
            mem_attr: MemAttr::Device,
            readable: true,
            writable: true,
            executable: false,
            user: false,
        }
    }
}

fn pte_attr_bits(flags: MapFlags) -> u64 {
    let mut bits: u64 = 0;
    bits |= match flags.mem_attr {
        MemAttr::NormalCacheable => PTE_NORMAL_WB,
        MemAttr::Device => PTE_DEVICE,
    };
    if flags.user {
        bits |= PTE_AP_USER; // AP[1]=1 (User mode accessible)
        bits |= PTE_USER;    // Ensure PTE_USER/PTE_AP_USER are fully set
        if !flags.writable {
            bits |= PTE_AP_RO; // AP[2]=1 (User Read-Only)
        }
    } else {
        // Kernel-only pages (AP[1]=0)
        if !flags.writable {
            bits |= PTE_AP_RO; // AP[2]=1 (Kernel Read-Only)
        }
    }
    if !flags.executable {
        bits |= PTE_XN;
        bits |= PTE_UXN; // User pages get Unprivileged Execute Never when not executable
    }
    bits
}

pub fn debug_walk_va(l0_pa: usize, va: usize) {
    unsafe {
        let l0_idx = va_l0_index(va);
        let l0e = read_pte(l0_pa, l0_idx);
        crate::log_info!("PTE_WALK", "VA={:#x} L0_PA={:#x} L0_IDX={} L0E={:#x}", va, l0_pa, l0_idx, l0e);
        if l0e & 1 == 0 { return; }
        
        let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
        let l1_idx = va_l1_index(va);
        let l1e = read_pte(l1_pa, l1_idx);
        crate::log_info!("PTE_WALK", "  L1_PA={:#x} L1_IDX={} L1E={:#x}", l1_pa, l1_idx, l1e);
        if l1e & 1 == 0 { return; }
        if l1e & 0b10 == 0 { return; }
        
        let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
        let l2_idx = va_l2_index(va);
        let l2e = read_pte(l2_pa, l2_idx);
        crate::log_info!("PTE_WALK", "    L2_PA={:#x} L2_IDX={} L2E={:#x}", l2_pa, l2_idx, l2e);
        if l2e & 1 == 0 { return; }
        if l2e & 0b10 == 0 { return; }
        
        let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
        let l3_idx = va_l3_index(va);
        let l3e = read_pte(l3_pa, l3_idx);
        crate::log_info!("PTE_WALK", "      L3_PA={:#x} L3_IDX={} L3E={:#x}", l3_pa, l3_idx, l3e);
    }
}

/// Read TTBR1_EL1 (kernel page table root).
#[inline(always)]
fn ttbr1_el1() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {0}, ttbr1_el1", out(reg) v, options(nomem, nostack)) };
    v
}

/// Read TTBR0_EL1 (user page table root).
#[inline(always)]
fn ttbr0_el1() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {0}, ttbr0_el1", out(reg) v, options(nomem, nostack)) };
    v
}

#[inline(always)]
fn root_for_va(va: usize) -> u64 {
    if va >= KERNEL_OFFSET {
        ttbr1_el1()
    } else {
        let t0 = ttbr0_el1();
        if t0 != 0 {
            t0
        } else {
            ttbr1_el1()
        }
    }
}

/// Shatter an L1 Block entry: replace the 1 GiB block with a fresh L2 table
/// containing 512 2 MiB block entries that re-create the original mapping.
unsafe fn shatter_l1_block(l1_pa: usize, l1_idx: usize, original: u64) -> Result<usize> {
    let new_l2_pa = phys::alloc_page()?.as_usize();
    zero_page(new_l2_pa);

    let block_base_pa = (original & 0x0000_FFFF_FFFF_F000) as usize;
    // Original attributes (AttrIdx, SH, AF, AP, XN, etc.)
    let mut block_attr = original & 0xFFF0_0000_0000_0FFF;
    // CRITICAL FIX: Ensure user-accessible bits are set so that any user address mapping
    // in this 1 GiB range is authorized at higher-level table translations!
    block_attr |= PTE_USER;
    // CRITICAL FIX: Remove execute-never (PTE_XN / PTE_UXN) from the shattered blocks if they are user regions,
    // so user executable code mapped inside shattered block space is allowed to run.
    block_attr &= !PTE_XN;
    block_attr &= !PTE_UXN;

    for i in 0..512 {
        let entry = pa_to_pte_addr(block_base_pa + i * 0x20_0000)
                  | PTE_VALID
                  | PTE_TYPE_BLOCK
                  | block_attr;
        write_pte(new_l2_pa, i, entry);
    }

    // Replace L1 block with a Table entry pointing at the new L2.
    let l1_entry = pa_to_pte_addr(new_l2_pa) | PTE_VALID | PTE_TYPE_TABLE;
    write_pte(l1_pa, l1_idx, l1_entry);
    Ok(new_l2_pa)
}

/// Shatter an L2 Block entry: replace the 2 MiB block with a fresh L3 table
/// containing 512 4 KiB page entries that re-create the original mapping.
unsafe fn shatter_l2_block(l2_pa: usize, l2_idx: usize, original: u64) -> Result<usize> {
    let new_l3_pa = phys::alloc_page()?.as_usize();
    zero_page(new_l3_pa);

    let block_pa = (original & 0x0000_FFFF_FFFF_F000) as usize;
    let mut block_attr = original & 0xFFF0_0000_0000_0FFF; // attr, AP, XN, AF, ...
    // CRITICAL FIX: Ensure user-accessible bits are set so that any user address mapping
    // in this 2 MiB range is authorized at higher-level table translations!
    block_attr |= PTE_AP_USER;
    // CRITICAL FIX: Remove execute-never (PTE_XN / PTE_UXN) from the shattered blocks so user executable code can run.
    block_attr &= !PTE_XN;
    block_attr &= !PTE_UXN;

    for i in 0..512 {
        let entry = pa_to_pte_addr(block_pa + i * 0x1000)
                  | PTE_VALID
                  | PTE_TYPE_PAGE
                  | block_attr;
        write_pte(new_l3_pa, i, entry);
    }

    let l2_entry = pa_to_pte_addr(new_l3_pa) | PTE_VALID | PTE_TYPE_TABLE;
    write_pte(l2_pa, l2_idx, l2_entry);
    Ok(new_l3_pa)
}

/// Map a single 4 KiB page.  May shatter existing L1/L2 block entries
/// transparently.
pub fn map_page(va: usize, pa: usize, flags: MapFlags) -> Result<()> {
    if va & 0xFFF != 0 || pa & 0xFFF != 0 {
        return Err(shared::status::Status::InvalidArgs);
    }

    unsafe {
        let root = root_for_va(va) as usize;
        let l0_pa = root & 0x0000_FFFF_FFFF_F000;
        let l0_idx = va_l0_index(va);
        let l0e = read_pte(l0_pa, l0_idx);
        let l1_pa = if l0e & PTE_VALID != 0 && l0e & 0b10 != 0 {
            // Table entry -> follow.
            (l0e & 0x0000_FFFF_FFFF_F000) as usize
        } else if l0e & PTE_VALID != 0 {
            // Block at L0: not used in our layout, but handle it.
            return Err(shared::status::Status::NotAllowed);
        } else {
            // Allocate L1.
            let new_l1 = phys::alloc_page()?.as_usize();
            zero_page(new_l1);
            // CRITICAL FIX: Ensure the Table Descriptor has UXNTable (bit 60), PXNTable (bit 61), 
            // and APTable (bits 62:61) set to 0. This allows lower levels (EL0 user) to fully execute code.
            let entry = pa_to_pte_addr(new_l1) | PTE_VALID | PTE_TYPE_TABLE;
            let clean_entry = entry & 0x07FF_FFFF_FFFF_FFFFu64;
            write_pte(l0_pa, l0_idx, clean_entry);
            new_l1
        };

        let l1_idx = va_l1_index(va);
        let l1e = read_pte(l1_pa, l1_idx);
        let l2_pa = if l1e & PTE_VALID != 0 && l1e & 0b10 != 0 {
            // Table entry -> follow.
            (l1e & 0x0000_FFFF_FFFF_F000) as usize
        } else if l1e & PTE_VALID != 0 {
            // Block entry -> shatter to L2 table.
            shatter_l1_block(l1_pa, l1_idx, l1e)?
        } else {
            let new_l2 = phys::alloc_page()?.as_usize();
            zero_page(new_l2);
            // CRITICAL FIX: Ensure UXNTable / PXNTable / APTable are zeroed out on this Table Descriptor
            let entry = pa_to_pte_addr(new_l2) | PTE_VALID | PTE_TYPE_TABLE;
            let clean_entry = entry & 0x07FF_FFFF_FFFF_FFFFu64;
            write_pte(l1_pa, l1_idx, clean_entry);
            new_l2
        };

        let l2_idx = va_l2_index(va);
        let l2e = read_pte(l2_pa, l2_idx);
        let l3_pa = if l2e & PTE_VALID != 0 && l2e & 0b10 != 0 {
            // Table entry -> follow.
            (l2e & 0x0000_FFFF_FFFF_F000) as usize
        } else if l2e & PTE_VALID != 0 {
            // Block entry -> shatter to L3 table.
            shatter_l2_block(l2_pa, l2_idx, l2e)?
        } else {
            let new_l3 = phys::alloc_page()?.as_usize();
            zero_page(new_l3);
            // CRITICAL FIX: Ensure UXNTable / PXNTable / APTable are zeroed out on this Table Descriptor
            let entry = pa_to_pte_addr(new_l3) | PTE_VALID | PTE_TYPE_TABLE;
            let clean_entry = entry & 0x07FF_FFFF_FFFF_FFFFu64;
            write_pte(l2_pa, l2_idx, clean_entry);
            new_l3
        };

        // Write the L3 page entry.
        let l3_idx = va_l3_index(va);
        let entry = pa_to_pte_addr(pa)
                  | PTE_VALID
                  | PTE_TYPE_PAGE
                  | pte_attr_bits(flags);
        write_pte(l3_pa, l3_idx, entry);
        // Clean+invalidate the cache line we just wrote so the MMU
        // hardware walk (which can re-read the PTE on the next access)
        // sees the new value rather than a stale cache line.  Without
        // this, the write-back cache can hold the new entry while the
        // MMU continues to use an older cached translation until eviction.
        if crate::mm::phys::mmu_is_active() {
            let line_va = pa_to_kernel_va(l3_pa) + l3_idx * 8;
            unsafe {
                core::arch::asm!("dc civac, {0}", in(reg) line_va, options(nomem, nostack));
                core::arch::asm!("dsb ish", options(nomem, nostack));
            }
        }

        // TLB invalidate single entry.
        asm!(
            "dsb ishst",
            "tlbi vaae1, {0}",
            "dsb ish",
            "isb",
            in(reg) (va >> 12),
            options(nomem, nostack)
        );
    }
    Ok(())
}

/// Unmap a single 4 KiB page.  Just clears the L3 PTE; the intermediate
/// tables are left in place (they will be reused for further mappings).
pub fn unmap_page(va: usize) -> Result<()> {
    if va & 0xFFF != 0 {
        return Err(shared::status::Status::InvalidArgs);
    }
    unsafe {
        let l0_pa = (root_for_va(va) as usize) & 0x0000_FFFF_FFFF_F000;
        let l0_idx = va_l0_index(va);
        let l0e = read_pte(l0_pa, l0_idx);
        if l0e & PTE_VALID == 0 || l0e & 0b10 == 0 {
            return Ok(()); // nothing mapped
        }
        let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
        let l1_idx = va_l1_index(va);
        let l1e = read_pte(l1_pa, l1_idx);
        if l1e & PTE_VALID == 0 || l1e & 0b10 == 0 {
            return Ok(());
        }
        let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
        let l2_idx = va_l2_index(va);
        let l2e = read_pte(l2_pa, l2_idx);
        if l2e & PTE_VALID == 0 || l2e & 0b10 == 0 {
            return Ok(());
        }
        let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
        let l3_idx = va_l3_index(va);
        write_pte(l3_pa, l3_idx, 0);

        asm!(
            "dsb ishst",
            "tlbi vaae1, {0}",
            "dsb ish",
            "isb",
            in(reg) (va >> 12),
            options(nomem, nostack)
        );
    }
    Ok(())
}

#[inline(always)]
pub fn set_ttbr0_el1(l0_pa: usize) {
    unsafe {
        core::arch::asm!("msr ttbr0_el1, {0}", in(reg) l0_pa as u64, options(nomem, nostack));
        core::arch::asm!("dsb ish", options(nomem, nostack));
        core::arch::asm!("tlbi vmalle1is", options(nomem, nostack));
        core::arch::asm!("dsb ish", options(nomem, nostack));
        core::arch::asm!("isb", options(nomem, nostack));
    }
}

pub fn build_and_enable(boot: &BootInfo) -> Result<()> {
    let _ = boot;
    unsafe { enable_inner(boot.ram_base, boot.ram_size, boot.uart_base) }
}

pub fn enable_inner(ram_base: usize, ram_size: usize, _uart_base: usize) -> Result<()> {
    unsafe {
        // Allocate 3 pages: L0, L1_ID (identity), L1_HIGH (high-half mirror of RAM).
        let l0_pa = phys::alloc_page()?.as_usize();
        let l1_id_pa = phys::alloc_page()?.as_usize();
        let l1_high_pa = phys::alloc_page()?.as_usize();
        zero_page(l0_pa);
        zero_page(l1_id_pa);
        zero_page(l1_high_pa);

        // Identity: 1 GiB blocks covering UART (low 1 GiB) and RAM.
        write_l1_block(l1_id_pa, 0, 0x0000_0000, MemAttr::Device);
        write_l1_block(l1_id_pa, 1, 0x4000_0000, MemAttr::NormalCacheable);

        // High-half mirror: 1 GiB block @ 0x4000_0000 reachable from the high half.
        let ram_va = pa_to_kernel_va(ram_base);
        let l0_idx_high = va_l0_index(ram_va);
        let l1_idx_high = va_l1_index(ram_va);
        write_l1_block(l1_high_pa, l1_idx_high, 0x4000_0000, MemAttr::NormalCacheable);

        // L0[0] -> L1_ID, L0[l0_idx_high] -> L1_HIGH.
        write_l0_table(l0_pa, 0, l1_id_pa);
        write_l0_table(l0_pa, l0_idx_high, l1_high_pa);

        // MAIR: index 0 = Normal WB (0xFF), index 1 = Device nGnRnE (0x00).
        let mair: u64 = 0xFFu64 | (0x00u64 << 8);
        // T0SZ=16, T1SZ=16, TG0=4K, TG1=4K, IPS=000 (32-bit PA, perfectly matching QEMU 512MB RAM),
        // SH0/1=ISH, ORGN/IRGN=WB.
        let tcr: u64 = (16u64 << 0)
                     | (16u64 << 16)
                     | (0b00u64 << 14)
                     | (0b10u64 << 30)
                     | (0b000u64 << 32)
                     | (0b11u64 << 12)
                     | (0b11u64 << 28)
                     | (0b01u64 << 10)
                     | (0b01u64 << 26)
                     | (0b01u64 << 8)
                     | (0b01u64 << 24);

        asm!("msr mair_el1, {0}", in(reg) mair, options(nomem, nostack));
        asm!("msr tcr_el1, {0}",  in(reg) tcr,  options(nomem, nostack));
        asm!("msr ttbr0_el1, {0}", in(reg) l0_pa as u64, options(nomem, nostack));
        asm!("msr ttbr1_el1, {0}", in(reg) l0_pa as u64, options(nomem, nostack));
        asm!("isb", options(nomem, nostack));

        // Enable MMU: set M + C + I bits, and ENSURE CPACR_EL1 FPEN is also fully preserved and set.
        // We set CPACR_EL1 explicitly to 0x300000 here to double-secure EL0 FP/SIMD.
        let mut cpacr: u64 = 0x300000;
        asm!("msr cpacr_el1, {0}", in(reg) cpacr, options(nomem, nostack));
        asm!("isb", options(nomem, nostack));

        let mut sctlr: u64;
        asm!("mrs {0}, sctlr_el1", out(reg) sctlr, options(nomem, nostack));
        sctlr |= 1u64 << 0;   // M
        sctlr |= 1u64 << 2;   // C
        sctlr |= 1u64 << 12;  // I
        sctlr &= !(1u64 << 25); // Disable EL0 Stack Alignment Check (SP0) to prevent traps
        sctlr &= !(1u64 << 1);  // Disable strict memory alignment checks (A)
        asm!("msr sctlr_el1, {0}", in(reg) sctlr, options(nomem, nostack));
        asm!("isb", options(nomem, nostack));
        // Invalidate all TLB entries.
        asm!("tlbi vmalle1", options(nomem, nostack));
        asm!("dsb sy", options(nomem, nostack));
        asm!("isb", options(nomem, nostack));

        let _ = ram_size;
    }
    crate::mm::phys::mark_mmu_active();
    Ok(())
}

pub struct AArch64Mmu;

impl AArch64Mmu {
    pub fn new() -> Self { AArch64Mmu }
    pub fn enable(&self) {}
    pub fn disable(&self) {}
    pub fn is_enabled() -> bool { false }
}

pub struct AArch64PageTable;
impl AArch64PageTable { pub fn new() -> Result<Self> { Ok(AArch64PageTable) } }

pub struct AArch64PageFlags(u64);
impl AArch64PageFlags {
    pub fn read() -> Self { Self(1 << 6) }
    pub fn write() -> Self { Self(1 << 7) }
    pub fn execute() -> Self { Self(0) }
    pub fn user() -> Self { Self(1 << 5) }
    pub fn kernel() -> Self { Self(0) }
    pub fn device() -> Self { Self(1 << 2) }
    pub fn none() -> Self { Self(0) }
    pub fn with_read(self) -> Self { Self(self.0 | (1 << 6)) }
    pub fn with_write(self) -> Self { Self(self.0 | (1 << 7)) }
    pub fn with_execute(self) -> Self { Self(self.0 | (1 << 8)) }
}

pub struct AArch64AddressSpace { _t: AArch64PageTable }
impl AArch64AddressSpace {
    pub fn new(_base: usize, _size: usize) -> Result<Self> { Ok(Self { _t: AArch64PageTable }) }
    pub fn activate(&self) {}
    pub fn table(&self) -> &AArch64PageTable { &self._t }
}

impl ArchMmu for AArch64Mmu {
    fn enable_with(boot: &BootInfo) {
        let _ = build_and_enable(boot);
    }
    fn flush_tlb_all() {
        unsafe {
            asm!("tlbi vmalle1", options(nomem, nostack));
            asm!("dsb sy", options(nomem, nostack));
            asm!("isb", options(nomem, nostack));
        }
    }
}

/// Dynamic cache coherency helper to clean data cache and invalidate instruction cache
/// on newly written/mapped EL0 user program segments.
pub fn sync_instruction_cache(va: usize, size: usize) {
    unsafe {
        let mut ctr: u64;
        core::arch::asm!("mrs {0}, ctr_el0", out(reg) ctr, options(nomem, nostack));
        
        let dlog2 = (ctr >> 16) & 0xf;
        let d_step = 4 << dlog2;
        
        let ilog2 = ctr & 0xf;
        let i_step = 4 << ilog2;

        let start = va;
        let end = va + size;

        let mut cur = start & !(d_step - 1);
        while cur < end {
            // Clean data cache to PoU
            core::arch::asm!("dc cvau, {0}", in(reg) cur, options(nomem, nostack));
            cur += d_step;
        }
        core::arch::asm!("dsb ish", options(nomem, nostack));
        
        cur = start & !(i_step - 1);
        while cur < end {
            // Invalidate instruction cache to PoU
            core::arch::asm!("ic ivau, {0}", in(reg) cur, options(nomem, nostack));
            cur += i_step;
        }
        core::arch::asm!("dsb ish", options(nomem, nostack));
        core::arch::asm!("isb", options(nomem, nostack));
    }
}

pub fn translate_user_va(l0_pa: usize, va: usize) -> Option<usize> {
    unsafe {
        let l0_idx = va_l0_index(va);
        let l0e = read_pte(l0_pa, l0_idx);
        if l0e & 1 == 0 {
            return None;
        }

        let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
        let l1_idx = va_l1_index(va);
        let l1e = read_pte(l1_pa, l1_idx);
        if l1e & 1 == 0 {
            return None;
        }
        if l1e & 0b10 == 0 {
            return None;
        }

        let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
        let l2_idx = va_l2_index(va);
        let l2e = read_pte(l2_pa, l2_idx);
        if l2e & 1 == 0 {
            return None;
        }
        if l2e & 0b10 == 0 {
            return None;
        }

        let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
        let l3_idx = va_l3_index(va);
        let l3e = read_pte(l3_pa, l3_idx);
        if l3e & 1 == 0 {
            return None;
        }

        let page_pa = (l3e & 0x0000_FFFF_FFFF_F000) as usize;
        let offset = va & 0xFFF;
        Some(page_pa + offset)
    }
}
