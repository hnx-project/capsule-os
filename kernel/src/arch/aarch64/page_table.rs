use shared::status::{Result, Status};
use crate::arch::ArchHardware;
use crate::arch::mmu_facade::{MemAttr, PAGE_SIZE, pa_to_kernel_va};
use crate::arch::aarch64::phys::{self, PhysAddr};

// ── AArch64 PTE constants ──────────────────────────────────────────
const PTE_VALID: u64 = 1 << 0;
const PTE_TYPE_BLOCK: u64 = 1;
const PTE_TYPE_TABLE: u64 = 3;
const PTE_TYPE_PAGE: u64 = 3;
const PTE_AF: u64 = 1 << 10;
const PTE_NSH: u64 = 0b00 << 8;
const PTE_ISH: u64 = 0b11 << 8;
const PTE_NORMAL_WB: u64 = (0u64 << 2) | PTE_ISH | PTE_AF;
const PTE_DEVICE: u64 = (1u64 << 2) | PTE_NSH | PTE_AF;
const PTE_USER: u64 = 1 << 6;
const PTE_AP_USER: u64 = 1 << 6;
const PTE_AP_RO: u64 = 1 << 7;
const PTE_USER_RW: u64 = PTE_AP_USER;
const PTE_USER_RO: u64 = PTE_AP_USER | PTE_AP_RO;
const PTE_XN: u64 = 1 << 54;
const PTE_UXN: u64 = 1 << 53;

// ── VA index helpers ───────────────────────────────────────────────
#[inline(always)]
fn va_l0_index(va: usize) -> usize { (va >> 39) & 0x1FF }
#[inline(always)]
pub fn va_l1_index(va: usize) -> usize { (va >> 30) & 0x1FF }
#[inline(always)]
pub fn va_l2_index(va: usize) -> usize { (va >> 21) & 0x1FF }
#[inline(always)]
pub fn va_l3_index(va: usize) -> usize { (va >> 12) & 0x1FF }

#[inline(always)]
fn pa_to_pte_addr(pa: usize) -> u64 { (pa as u64) & 0x0000_FFFF_FFFF_F000 }

fn mmu_active() -> bool { phys::mmu_is_active() }

// ── Low-level PTE helpers ──────────────────────────────────────────
pub unsafe fn read_pte(table_pa: usize, idx: usize) -> u64 {
    let ptr = if mmu_active() {
        pa_to_kernel_va(table_pa) as *mut u64
    } else {
        table_pa as *mut u64
    };
    core::ptr::read_volatile(ptr.add(idx))
}

unsafe fn write_pte(table_pa: usize, idx: usize, entry: u64) {
    let ptr = if mmu_active() {
        pa_to_kernel_va(table_pa) as *mut u64
    } else {
        table_pa as *mut u64
    };
    core::ptr::write_volatile(ptr.add(idx), entry);
    if mmu_active() {
        let line_va = pa_to_kernel_va(table_pa) + idx * 8;
        unsafe {
            <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::clean_and_invalidate_cache_range(line_va, 8);
        }
    }
}

unsafe fn flush_table_page(table_pa: usize) {
    if !mmu_active() { return; }
    let base = pa_to_kernel_va(table_pa);
    unsafe {
        <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::clean_and_invalidate_cache_range(base, PAGE_SIZE);
    }
}

fn pte_attr_bits(flags: &crate::arch::mmu::MapFlags) -> u64 {
    let mut bits: u64 = 0;
    bits |= match flags.mem_attr {
        MemAttr::NormalCacheable => PTE_NORMAL_WB,
        MemAttr::Device => PTE_DEVICE,
    };
    if flags.user {
        if flags.writable {
            bits |= PTE_USER_RW;
        } else {
            bits |= PTE_USER_RO;
        }
    } else if !flags.writable {
        bits |= PTE_AP_USER;
    }

    if !flags.executable {
        bits |= PTE_XN;
        bits |= PTE_UXN;
    }

    bits |= PTE_AF;
    bits
}

// ── PageTableTree (AArch64-specific raw hardware mappings) ─────────
#[derive(Debug)]
pub struct PageTableTree {
    l0_pa: usize,
    tracked: [usize; 64],
    count: usize,
    pub generation: u64,
    has_root: bool,
}

impl PageTableTree {
    pub const fn new() -> Self {
        Self {
            l0_pa: 0,
            tracked: [0; 64],
            count: 0,
            generation: 0,
            has_root: false,
        }
    }

    pub fn allocate_root(&mut self) -> Result<()> {
        if self.has_root {
            return Err(Status::AlreadyExists);
        }
        unsafe {
            let l0 = phys::alloc_pt_page()?.as_usize();
            let l1 = phys::alloc_pt_page()?.as_usize();

            flush_table_page(l0);
            flush_table_page(l1);

            let l0_kva = pa_to_kernel_va(l0) as *mut u64;
            let l1_entry = pa_to_pte_addr(l1) | PTE_VALID | PTE_TYPE_TABLE;
            let clean_entry = l1_entry & 0x0000_FFFF_FFFF_F003u64;

            core::ptr::write_volatile(l0_kva, clean_entry);
            unsafe {
                <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::clean_cache_range(l0_kva as usize, 8);
            }

            self.l0_pa = l0;
            self.tracked[0] = l0;
            self.tracked[1] = l1;
            self.count = 2;
            self.generation = 1;
            self.has_root = true;
        }
        Ok(())
    }

    pub fn set_root_raw(&mut self, l0_pa: usize) {
        self.l0_pa = l0_pa;
        self.has_root = true;
        self.generation = self.generation.wrapping_add(1);
        self.track(l0_pa);
    }

    pub fn l0_pa(&self) -> usize { self.l0_pa }
    pub fn page_count(&self) -> usize { self.count }
    pub fn has_root(&self) -> bool { self.has_root }

    pub fn track(&mut self, pa: usize) {
        if self.count >= 64 { return; }
        for i in 0..self.count {
            if self.tracked[i] == pa { return; }
        }
        self.tracked[self.count] = pa;
        self.count += 1;
        crate::arch::aarch64::phys::register_pt_page(pa);
    }

    pub fn clone_user_from(&mut self, parent_l0_pa: usize) {
        // S3 fork: walk every user-space L0 entry (slots 0..256),
        // and for each populated entry, allocate a fresh sub-table
        // (L1/L2/L3) and copy every leaf PTE into the child's
        // page tables.  Physical pages are shared between parent
        // and child through a single physical-frame reference so
        // subsequent writes from either side will mutate the same
        // backstore.
        //
        // This is the "shallow clone" used by Pangu 1.0 fork:
        // cheap enough that bash's `fork + exec` pattern costs
        // < 10ms on QEMU, but a future S11/S12 can replace it
        // with proper COW + write-fault handling by tracking a
        // refcount on each physical frame.
        if !self.has_root {
            return;
        }
        if parent_l0_pa == 0 || parent_l0_pa == self.l0_pa {
            return;
        }
        unsafe {
            let parent_l0 = pa_to_kernel_va(parent_l0_pa) as *const u64;
            let child_l0 = pa_to_kernel_va(self.l0_pa) as *mut u64;
            // The kernel high-half entries (slot 256..512) are
            // shared; copy them now so the new process inherits
            // the kernel mapping.
            for idx in 256..512 {
                let entry = core::ptr::read_volatile(parent_l0.add(idx));
                if entry != 0 {
                    core::ptr::write_volatile(child_l0.add(idx), entry);
                }
            }
            // Walk user L0 entries (0..256).
            for idx in 0..256 {
                let l0_entry = core::ptr::read_volatile(parent_l0.add(idx));
                if l0_entry == 0 || l0_entry & PTE_VALID == 0 {
                    continue;
                }
                if l0_entry & 0b10 != 0 {
                    // 1 GiB block — not used by CapsuleOS, skip.
                    continue;
                }
                let parent_l1_pa = (l0_entry & 0x0000_FFFF_FFFF_F000) as usize;
                // Allocate a fresh L1 sub-table for the child.
                let new_l1_pa = match phys::alloc_pt_page() {
                    Ok(pa) => pa.as_usize(),
                    Err(_) => continue,
                };
                flush_table_page(new_l1_pa);
                self.track(new_l1_pa);
                // Wire the child's L0 entry.
                let new_l0_entry = pa_to_pte_addr(new_l1_pa) | PTE_VALID | PTE_TYPE_TABLE;
                let clean_new_l0 = new_l0_entry & 0x0000_FFFF_FFFF_F003u64;
                core::ptr::write_volatile(child_l0.add(idx), clean_new_l0);

                let parent_l1 = pa_to_kernel_va(parent_l1_pa) as *const u64;
                let child_l1 = pa_to_kernel_va(new_l1_pa) as *mut u64;

                for l1_idx in 0..512 {
                    let l1_entry = core::ptr::read_volatile(parent_l1.add(l1_idx));
                    if l1_entry == 0 || l1_entry & PTE_VALID == 0 {
                        continue;
                    }
                    if l1_entry & 0b10 != 0 {
                        // 2 MiB block — not used by CapsuleOS,
                        // skip.
                        continue;
                    }
                    let parent_l2_pa = (l1_entry & 0x0000_FFFF_FFFF_F000) as usize;
                    let new_l2_pa = match phys::alloc_pt_page() {
                        Ok(pa) => pa.as_usize(),
                        Err(_) => continue,
                    };
                    flush_table_page(new_l2_pa);
                    self.track(new_l2_pa);
                    let new_l1_entry =
                        pa_to_pte_addr(new_l2_pa) | PTE_VALID | PTE_TYPE_TABLE;
                    let clean_new_l1 = new_l1_entry & 0x0000_FFFF_FFFF_F003u64;
                    core::ptr::write_volatile(child_l1.add(l1_idx), clean_new_l1);

                    let parent_l2 = pa_to_kernel_va(parent_l2_pa) as *const u64;
                    let child_l2 = pa_to_kernel_va(new_l2_pa) as *mut u64;

                    for l2_idx in 0..512 {
                        let l2_entry = core::ptr::read_volatile(parent_l2.add(l2_idx));
                        if l2_entry == 0 || l2_entry & PTE_VALID == 0 {
                            continue;
                        }
                        let parent_l3_pa = (l2_entry & 0x0000_FFFF_FFFF_F000) as usize;
                        let new_l3_pa = match phys::alloc_pt_page() {
                            Ok(pa) => pa.as_usize(),
                            Err(_) => continue,
                        };
                        flush_table_page(new_l3_pa);
                        self.track(new_l3_pa);
                        let new_l2_entry =
                            pa_to_pte_addr(new_l3_pa) | PTE_VALID | PTE_TYPE_TABLE;
                        let clean_new_l2 = new_l2_entry & 0x0000_FFFF_FFFF_F003u64;
                        core::ptr::write_volatile(child_l2.add(l2_idx), clean_new_l2);

                        let parent_l3 = pa_to_kernel_va(parent_l3_pa) as *const u64;
                        let child_l3 = pa_to_kernel_va(new_l3_pa) as *mut u64;
                        // Copy every 4 KiB leaf PTE verbatim —
                        // the leaf PTE points at a physical page
                        // frame that continues to be shared between
                        // parent and child until one of them
                        // execve/exits.
                        for l3_idx in 0..512 {
                            let l3_entry =
                                core::ptr::read_volatile(parent_l3.add(l3_idx));
                            if l3_entry != 0 {
                                core::ptr::write_volatile(child_l3.add(l3_idx), l3_entry);
                            }
                        }
                    }
                }
            }
            // Make sure the new L0/L1/L2 tables are visible to the
            // page-table walker.
            <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::flush_tlb();
        }
    }

    pub fn clone_high_half(&mut self, parent_l0_pa: usize) {
        if !self.has_root { return; }
        unsafe {
            let src = pa_to_kernel_va(parent_l0_pa) as *const u64;
            let dst = pa_to_kernel_va(self.l0_pa) as *mut u64;
            for idx in 256..512 {
                let entry = core::ptr::read_volatile(src.add(idx));
                if entry != 0 {
                    core::ptr::write_volatile(dst.add(idx), entry);
                }
            }
            unsafe {
                <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::clean_cache_range(dst as usize, 512 * 8);
            }
        }
    }

    pub fn clone_identity_block(&mut self, parent_l0_pa: usize) {
        if !self.has_root { return; }
        unsafe {
            let parent_l0_kva = pa_to_kernel_va(parent_l0_pa) as *const u64;
            let entry_0 = core::ptr::read_volatile(parent_l0_kva.add(0));
            if entry_0 == 0 { return; }
            let parent_l1_pa = (entry_0 & 0x0000_FFFF_FFFF_F000) as usize;
            if parent_l1_pa == 0 { return; }
            let parent_l1_kva = pa_to_kernel_va(parent_l1_pa) as *const u64;

            let my_l0_kva = pa_to_kernel_va(self.l0_pa) as *const u64;
            let my_entry_0 = core::ptr::read_volatile(my_l0_kva.add(0));
            if my_entry_0 == 0 { return; }
            let my_l1_pa = (my_entry_0 & 0x0000_FFFF_FFFF_F000) as usize;
            if my_l1_pa == 0 { return; }
            let my_l1_kva = pa_to_kernel_va(my_l1_pa) as *mut u64;

            for i in 0..3 {
                if i == 0 || i == 2 { continue; }
                let entry = core::ptr::read_volatile(parent_l1_kva.add(i));
                if entry != 0 {
                    core::ptr::write_volatile(my_l1_kva.add(i), entry);
                }
            }
        }
    }

    pub fn map_va(&mut self, va: usize, pa: usize, flags: &crate::arch::mmu::MapFlags) -> Result<()> {
        if va & 0xFFF != 0 || pa & 0xFFF != 0 {
            return Err(Status::InvalidArgs);
        }
        if !self.has_root {
            return Err(Status::InvalidArgs);
        }

        unsafe {
            let l0_pa = self.l0_pa;
            let l0_idx = va_l0_index(va);
            let l0e = read_pte(l0_pa, l0_idx);
            let l1_pa = if l0e & PTE_VALID != 0 && l0e & 0b10 != 0 {
                (l0e & 0x0000_FFFF_FFFF_F000) as usize
            } else if l0e & PTE_VALID != 0 {
                return Err(Status::NotAllowed);
            } else {
                let new_l1 = phys::alloc_pt_page()?.as_usize();
                flush_table_page(new_l1);
                let entry = pa_to_pte_addr(new_l1) | PTE_VALID | PTE_TYPE_TABLE;
                let clean_entry = entry & 0x0000_FFFF_FFFF_F003u64;
                write_pte(l0_pa, l0_idx, clean_entry);
                self.track(new_l1);
                new_l1
            };

            let l1_idx = va_l1_index(va);
            let l1e = read_pte(l1_pa, l1_idx);
            let l2_pa = if l1e & PTE_VALID != 0 && l1e & 0b10 != 0 {
                (l1e & 0x0000_FFFF_FFFF_F000) as usize
            } else if l1e & PTE_VALID != 0 {
                self.shatter_l1_block(l1_pa, l1_idx, l1e)?
            } else {
                let new_l2 = phys::alloc_pt_page()?.as_usize();
                flush_table_page(new_l2);
                let entry = pa_to_pte_addr(new_l2) | PTE_VALID | PTE_TYPE_TABLE;
                let clean_entry = entry & 0x0000_FFFF_FFFF_F003u64;
                write_pte(l1_pa, l1_idx, clean_entry);
                self.track(new_l2);
                new_l2
            };

            let l2_idx = va_l2_index(va);
            let l2e = read_pte(l2_pa, l2_idx);
            let l3_pa = if l2e & PTE_VALID != 0 && l2e & 0b10 != 0 {
                (l2e & 0x0000_FFFF_FFFF_F000) as usize
            } else if l2e & PTE_VALID != 0 {
                self.shatter_l2_block(l2_pa, l2_idx, l2e)?
            } else {
                let new_l3 = phys::alloc_pt_page()?.as_usize();
                flush_table_page(new_l3);
                let entry = pa_to_pte_addr(new_l3) | PTE_VALID | PTE_TYPE_TABLE;
                let clean_entry = entry & 0x0000_FFFF_FFFF_F003u64;
                write_pte(l2_pa, l2_idx, clean_entry);
                self.track(new_l3);
                new_l3
            };

            let l3_idx = va_l3_index(va);
            let entry = pa_to_pte_addr(pa)
                      | PTE_VALID
                      | PTE_TYPE_PAGE
                      | pte_attr_bits(flags);
            write_pte(l3_pa, l3_idx, entry);
            if mmu_active() {
                let line_va = pa_to_kernel_va(l3_pa) + l3_idx * 8;
                unsafe {
                    <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::clean_and_invalidate_cache_range(line_va, 8);
                }
            }

            unsafe {
                <crate::arch::aarch64::Aarch64Hardware as ArchHardware>::flush_tlb();
            }
        }
        Ok(())
    }

    pub fn translate_va(&self, va: usize) -> Option<usize> {
        if !self.has_root { return None; }
        unsafe {
            let l0_idx = va_l0_index(va);
            let l0e = read_pte(self.l0_pa, l0_idx);
            if l0e & 1 == 0 { return None; }

            let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
            let l1_idx = va_l1_index(va);
            let l1e = read_pte(l1_pa, l1_idx);
            if l1e & 1 == 0 { return None; }
            if l1e & 0b10 == 0 {
                let block_pa = (l1e & 0x0000_FFFF_C000_0000) as usize;
                return Some(block_pa + (va & 0x3FFF_FFFF));
            }

            let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
            let l2_idx = va_l2_index(va);
            let l2e = read_pte(l2_pa, l2_idx);
            if l2e & 1 == 0 { return None; }
            if l2e & 0b10 == 0 {
                let block_pa = (l2e & 0x0000_FFFF_FFE0_0000) as usize;
                return Some(block_pa + (va & 0x1F_FFFF));
            }

            let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
            let l3_idx = va_l3_index(va);
            let l3e = read_pte(l3_pa, l3_idx);
            if l3e & 1 == 0 { return None; }

            let page_pa = (l3e & 0x0000_FFFF_FFFF_F000) as usize;
            Some(page_pa + (va & (PAGE_SIZE - 1)))
        }
    }

    pub unsafe fn free_tree(&mut self) {
        if !self.has_root { return; }
        for idx in (0..self.count).rev() {
            let pa = self.tracked[idx];
            if pa != 0 {
                phys::free_page(PhysAddr::new(pa));
            }
        }
        self.l0_pa = 0;
        self.count = 0;
        self.generation = 0;
        self.has_root = false;
    }

    pub fn validate(&self) -> bool {
        if !self.has_root {
            return true;
        }

        let mut ok = true;
        let mut reachable: [usize; 64] = [0; 64];
        let mut reachable_count = 0;

        let mut track_reachable = |pa: usize| {
            if reachable_count >= 64 { return; }
            for i in 0..reachable_count {
                if reachable[i] == pa { return; }
            }
            reachable[reachable_count] = pa;
            reachable_count += 1;
        };

        unsafe {
            track_reachable(self.l0_pa);

            for l0_idx in 0..256 {
                let l0e = read_pte(self.l0_pa, l0_idx);
                if l0e & PTE_VALID == 0 { continue; }
                if l0e & 0b10 == 0 {
                    crate::log_error!("PT-VALIDATE", "L0[{}] is a block entry", l0_idx);
                    ok = false;
                    continue;
                }
                let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
                track_reachable(l1_pa);

                for l1_idx in 0..512 {
                    let l1e = read_pte(l1_pa, l1_idx);
                    if l1e & PTE_VALID == 0 { continue; }
                    if l1e & 0b10 == 0 { continue; }
                    let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
                    track_reachable(l2_pa);

                    for l2_idx in 0..512 {
                        let l2e = read_pte(l2_pa, l2_idx);
                        if l2e & PTE_VALID == 0 { continue; }
                        if l2e & 0b10 == 0 { continue; }
                        let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
                        track_reachable(l3_pa);

                        for l3_idx in 0..512 {
                            let l3e = read_pte(l3_pa, l3_idx);
                            if l3e & PTE_VALID == 0 { continue; }
                            if l3e & 0b10 == 0 {
                                crate::log_error!("PT-VALIDATE", "L3[{}] is not a page entry", l3_idx);
                                ok = false;
                            }
                        }
                    }
                }
            }

            for i in 0..self.count {
                let pa = self.tracked[i];
                if pa != 0 {
                    let mut found = false;
                    for j in 0..reachable_count {
                        if reachable[j] == pa { found = true; break; }
                    }
                    if !found {
                        crate::log_error!(
                            "PT-VALIDATE",
                            "tracked[{}]={:#x} NOT reachable from L0 (l0_pa={:#x})",
                            i, pa, self.l0_pa
                        );
                        ok = false;
                    }
                }
            }

            for i in 0..reachable_count {
                let pa = reachable[i];
                let mut found = false;
                for j in 0..self.count {
                    if self.tracked[j] == pa { found = true; break; }
                }
                if !found {
                    crate::log_error!(
                        "PT-VALIDATE",
                        "reachable PA {:#x} NOT in tracked[] (l0_pa={:#x})",
                        pa, self.l0_pa
                    );
                    ok = false;
                }
            }
        }

        ok
    }
}

impl PageTableTree {
    unsafe fn shatter_l1_block(&mut self, l1_pa: usize, l1_idx: usize, original: u64) -> Result<usize> {
        let new_l2_pa = phys::alloc_pt_page()?.as_usize();
        self.track(new_l2_pa);

        let block_base_pa = (original & 0x0000_FFFF_FFFF_F000) as usize;
        let mut block_attr = original & 0x000F_0000_0000_0FFF;
        block_attr |= PTE_USER;
        block_attr &= !PTE_XN;
        block_attr &= !PTE_UXN;

        for i in 0..512 {
            let entry = pa_to_pte_addr(block_base_pa + i * 0x20_0000)
                      | PTE_VALID
                      | PTE_TYPE_BLOCK
                      | block_attr;
            write_pte(new_l2_pa, i, entry);
        }

        let l1_entry = pa_to_pte_addr(new_l2_pa) | PTE_VALID | PTE_TYPE_TABLE;
        let clean_l1_entry = l1_entry & 0x0000_FFFF_FFFF_F003u64;
        write_pte(l1_pa, l1_idx, clean_l1_entry);
        flush_table_page(new_l2_pa);
        Ok(new_l2_pa)
    }

    unsafe fn shatter_l2_block(&mut self, l2_pa: usize, l2_idx: usize, original: u64) -> Result<usize> {
        let new_l3_pa = phys::alloc_pt_page()?.as_usize();
        self.track(new_l3_pa);

        let block_pa = (original & 0x0000_FFFF_FFFF_F000) as usize;
        let mut block_attr = original & 0x000F_0000_0000_0FFF;
        block_attr |= PTE_AP_USER;
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
        let clean_l2_entry = l2_entry & 0x0000_FFFF_FFFF_F003u64;
        write_pte(l2_pa, l2_idx, clean_l2_entry);
        flush_table_page(new_l3_pa);
        Ok(new_l3_pa)
    }
}

impl crate::arch::ArchPageTable for PageTableTree {
    fn new() -> Self {
        Self::new()
    }

    fn allocate_root(&mut self) -> Result<()> {
        Self::allocate_root(self)
    }

    fn set_root_raw(&mut self, l0_pa: usize) {
        Self::set_root_raw(self, l0_pa);
    }

    fn has_root(&self) -> bool {
        self.has_root
    }

    fn l0_pa(&self) -> usize {
        self.l0_pa
    }

    fn track(&mut self, pa: usize) {
        Self::track(self, pa);
    }

    fn validate(&self) -> bool {
        Self::validate(self)
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn set_generation(&mut self, gen: u64) {
        self.generation = gen;
    }

    unsafe fn free_tree(&mut self) {
        Self::free_tree(self);
    }

    fn map_va(&mut self, va: usize, pa: usize, flags: &crate::arch::mmu::MapFlags) -> Result<()> {
        Self::map_va(self, va, pa, flags)
    }

    fn translate_va(&self, va: usize) -> Option<usize> {
        Self::translate_va(self, va)
    }

    fn clone_high_half(&mut self, src_l0_pa: usize) {
        Self::clone_high_half(self, src_l0_pa);
    }

    fn clone_identity_block(&mut self, src_l0_pa: usize) {
        Self::clone_identity_block(self, src_l0_pa);
    }

    fn clone_user_from(&mut self, src_l0_pa: usize) {
        Self::clone_user_from(self, src_l0_pa);
    }
}
