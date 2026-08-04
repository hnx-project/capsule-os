//! # 💊 Pangu-Hybrid Pill Loader (KLD - Kernel Linker & Loader)
//!
//! Exposes the primary kernel-level loading and relocation engine for dynamic
//! high-privilege ".pill" extension modules (KEXTs).

use shared::status::{Result, Status};
use crate::task::thread::Thread;
use crate::arch::mmu::MapFlags;
use crate::arch::mmu_facade::pa_to_kernel_va;
use crate::arch::phys::{alloc_page, PageTag};
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::arch::ArchHardware;
use ohlink_format::parser::OHLK_Parser;
use ohlink_format::symbol::OHLK_Symbol;
use ohlink_format::reloc::OHLK_Reloc;

/// Next available virtual address base in the kernel space (EL1 VMAR range)
/// dedicated to Pills, starting at 0xffff_9000_0000_0000.
/// Each Pill is allocated a safe, non-overlapping 256 MB window.
static NEXT_PILL_VA: AtomicUsize = AtomicUsize::new(0xffff_9000_0000_0000);

/// Core kernel exported symbol descriptor.
pub struct ExportSymbol {
    pub name: &'static str,
    pub addr: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PillQueueHandles {
    pub desc_vmo: u64,
    pub avail_vmo: u64,
    pub used_vmo: u64,
    pub desc_bytes: u64,
    pub avail_bytes: u64,
    pub used_bytes: u64,
    pub qsize: u32,
}

#[no_mangle]
pub extern "C" fn k_pa_to_kernel_va(pa: usize) -> usize {
    pa_to_kernel_va(pa)
}

#[no_mangle]
pub extern "C" fn kernel_virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<PillQueueHandles> {
    let q = qsize as usize;
    let desc_b = 16 * q;
    let avail_b = 6 + 2 * q;
    let mut used_b = 6 + 8 * q;
    if used_b < 4096 {
        used_b = 4096;
    }

    let desc_pa = alloc_page(PageTag::VmoData)?.as_usize();
    let avail_pa = alloc_page(PageTag::VmoData)?.as_usize();
    let used_pa = alloc_page(PageTag::VmoData)?.as_usize();

    unsafe {
        core::ptr::write_bytes(pa_to_kernel_va(desc_pa) as *mut u8, 0, 4096);
        core::ptr::write_bytes(pa_to_kernel_va(avail_pa) as *mut u8, 0, 4096);
        core::ptr::write_bytes(pa_to_kernel_va(used_pa) as *mut u8, 0, 4096);
    }

    Ok(PillQueueHandles {
        desc_vmo: desc_pa as u64,
        avail_vmo: avail_pa as u64,
        used_vmo: used_pa as u64,
        desc_bytes: desc_b as u64,
        avail_bytes: avail_b as u64,
        used_bytes: used_b as u64,
        qsize: qsize as u32,
    })
}

#[no_mangle]
pub extern "C" fn kernel_virtio_kick(slot: u32, qsel: u16) -> Result<()> {
    crate::drivers::bus::mmio_bus::sys_virtio_kick(slot, qsel)
}

#[no_mangle]
pub extern "C" fn kernel_virtio_read_isr(slot: u32) -> Result<u32> {
    crate::drivers::bus::mmio_bus::sys_virtio_read_isr(slot)
}

#[no_mangle]
pub extern "C" fn kernel_vmar_map_self(vmo_handle: usize, target_va: usize, len: usize) -> Result<()> {
    let page_count = (len + 4095) / 4096;
    for i in 0..page_count {
        let va = target_va + i * 4096;
        let pa = vmo_handle + i * 4096;
        unsafe {
            crate::arch::mmu::map_page(va, pa, MapFlags::kernel_rw())?;
        }
    }
    unsafe {
        crate::arch::aarch64::Aarch64Hardware::flush_tlb_local();
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn kernel_display_flush(_vmo: usize) {}

#[no_mangle]
pub extern "C" fn kernel_thread_yield() {
    unsafe {
        crate::task::scheduler::SCHEDULER.schedule();
    }
}

#[no_mangle]
pub extern "C" fn kernel_log(tag_ptr: *const u8, tag_len: usize, msg_ptr: *const u8, msg_len: usize) {
    let tag = unsafe { core::str::from_utf8(core::slice::from_raw_parts(tag_ptr, tag_len)).unwrap_or("PILL") };
    let msg = unsafe { core::str::from_utf8(core::slice::from_raw_parts(msg_ptr, msg_len)).unwrap_or("") };
    crate::log_info!("PILL", "[{}] {}", tag, msg);
}

/// Helper to lookup an exported kernel symbol address by its string name.
fn lookup_kernel_symbol(name: &str) -> Option<usize> {
    match name {
        "console_putchar" => Some(crate::arch::console_putchar as usize),
        "console_putbytes" => Some(crate::arch::console_putbytes as usize),
        "console_getchar" => Some(crate::arch::console_getchar as usize),
        "get_ticks" => Some(crate::arch::get_ticks as usize),
        "timer_phys_count" => Some(crate::arch::timer_phys_count as usize),
        "timer_freq_hz" => Some(crate::arch::timer_freq_hz as usize),
        "pa_to_kernel_va" => Some(k_pa_to_kernel_va as usize),
        "kernel_virtio_setup_queue" => Some(kernel_virtio_setup_queue as usize),
        "kernel_virtio_kick" => Some(kernel_virtio_kick as usize),
        "kernel_virtio_read_isr" => Some(kernel_virtio_read_isr as usize),
        "kernel_vmar_map_self" => Some(kernel_vmar_map_self as usize),
        "kernel_display_flush" => Some(kernel_display_flush as usize),
        "kernel_thread_yield" => Some(kernel_thread_yield as usize),
        "kernel_log" => Some(kernel_log as usize),
        _ => None,
    }
}

/// Parse, allocate, load, dynamically relocate, and launch a .pill driver package
/// inside the kernel virtual address space (EL1).
pub fn load_pill(pill_name: &'static str, binary_bytes: &[u8]) -> Result<u64> {
    crate::log_info!("PILL-LOADER", "Attempting to load Pill module '{}' ({} bytes)...", pill_name, binary_bytes.len());

    // 1. Verify binary integrity and parse header
    let parser = OHLK_Parser::new(binary_bytes).map_err(|e| {
        crate::log_error!("PILL-LOADER", "Failed to parse OHLINK binary headers: {:?}", e);
        Status::InvalidArgs
    })?;

    let header = parser.header();
    let base_pill_va = NEXT_PILL_VA.fetch_add(256 * 1024 * 1024, Ordering::Relaxed);

    crate::log_info!("PILL-LOADER", "Assigned base virtual address: {:#x}", base_pill_va);

    // 2. Identify and gather special helper segments (Symbol table, String table, Relocation table)
    let mut symtab_entry = None;
    let mut strtab_entry = None;
    let mut reloc_entry = None;

    for idx in 0..header.header_count {
        if let Ok(entry) = parser.get_entry(idx) {
            match entry.ty {
                5 => symtab_entry = Some(entry), // Symtab
                6 => strtab_entry = Some(entry), // Strtab
                7 => reloc_entry = Some(entry),  // Reloc
                _ => {}
            }
        }
    }

    let symtab_data = if let Some(entry) = &symtab_entry {
        parser.get_segment_data(entry).ok()
    } else {
        None
    };

    let strtab_data = if let Some(entry) = &strtab_entry {
        parser.get_segment_data(entry).ok()
    } else {
        None
    };

    let reloc_data = if let Some(entry) = &reloc_entry {
        parser.get_segment_data(entry).ok()
    } else {
        None
    };

    // 3. Load and map Text, Data, Rodata, and Bss segments
    for idx in 0..header.header_count {
        let entry = match parser.get_entry(idx) {
            Ok(e) => e,
            _ => continue,
        };

        let ty = entry.ty;
        if ty != 1 && ty != 2 && ty != 3 && ty != 4 {
            // Only process Text, Data, Rodata, Bss loadable segments
            continue;
        }

        let seg_offset = entry.virtual_address as usize;
        let seg_size = entry.mem_size as usize;
        let target_va = base_pill_va + seg_offset;

        let aligned_va = target_va & !(4096 - 1);
        let alignment_offset = target_va - aligned_va;
        let aligned_size = (seg_size + alignment_offset + 4095) & !(4095);

        // Strict W^X memory permissions
        let m_flags = if ty == 1 {
            // Text: Privileged RX (Read-Only, Executable)
            crate::arch::mmu::MapFlags::kernel_rx()
        } else {
            // Data, Rodata, Bss: Privileged RW (Read-Write, Non-Executable)
            crate::arch::mmu::MapFlags::kernel_rw()
        };

        let page_count = aligned_size / 4096;
        for i in 0..page_count {
            let page_va = aligned_va + i * 4096;
            
            // Allocate contiguous page in physical memory
            let page_pa = alloc_page(PageTag::VmoData)?.as_usize();
            
            // Map page into current active kernel TTBR1_EL1 page table
            unsafe {
                crate::arch::mmu::map_page(page_va, page_pa, m_flags)?;
            }

            // Zero-initialise memory page
            let page_kva = pa_to_kernel_va(page_pa) as *mut u8;
            unsafe {
                core::ptr::write_bytes(page_kva, 0, 4096);
            }
        }

        // Copy binary payload into allocated pages
        if entry.file_size > 0 {
            if let Ok(segment_payload) = parser.get_segment_data(&entry) {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        segment_payload.as_ptr(),
                        target_va as *mut u8,
                        segment_payload.len(),
                    );
                }
            }
        }
    }

    // 4. Perform dynamic linking and symbol relocation (KLD Engine)
    if let (Some(relocs), Some(syms), Some(strs)) = (reloc_data, symtab_data, strtab_data) {
        let reloc_count = relocs.len() / OHLK_Reloc::SIZE;
        crate::log_info!("PILL-LOADER", "Resolving and applying {} relocation entries...", reloc_count);

        for i in 0..reloc_count {
            let offset = i * OHLK_Reloc::SIZE;
            let reloc = OHLK_Reloc::from_bytes(&relocs[offset..offset + OHLK_Reloc::SIZE]).unwrap();

            // P: Absolute address of the relocation target point in kernel memory
            let p_va = base_pill_va + reloc.offset as usize;

            // Lookup the dependent symbol
            let sym_offset = reloc.symbol_idx as usize * OHLK_Symbol::SIZE;
            if sym_offset + OHLK_Symbol::SIZE > syms.len() {
                continue;
            }
            let symbol = OHLK_Symbol::from_bytes(&syms[sym_offset..sym_offset + OHLK_Symbol::SIZE]).unwrap();

            // Find symbol name in string table
            let name_start = symbol.name_offset as usize;
            let mut name_end = name_start;
            while name_end < strs.len() && strs[name_end] != 0 {
                name_end += 1;
            }
            let symbol_name = core::str::from_utf8(&strs[name_start..name_end]).unwrap_or("");

            // S: Target absolute virtual address of the resolved symbol
            let s_va = if symbol.section_idx != OHLK_Symbol::UNDEFINED_SECTION && symbol.value != 0 {
                // Internal Pill symbol: base address + offset
                base_pill_va + symbol.value as usize
            } else {
                // External kernel symbol: lookup in KERNEL_EXPORT_SYMBOLS
                if let Some(addr) = lookup_kernel_symbol(symbol_name) {
                    addr
                } else {
                    crate::log_error!(
                        "PILL-LOADER",
                        "[LINK ERROR] Symbol '{}' required by Pill '{}' is not exported by the kernel!",
                        symbol_name,
                        pill_name
                    );
                    return Err(Status::NotFound);
                }
            };

            let a = reloc.addend; // Explicit Addend

            // Apply specific ARM64 relocation formulas matching OHLINK Spec §1.1
            unsafe {
                match reloc.ty {
                    1 => {
                        // R_AARCH64_ABS64: direct 64-bit absolute value replacement
                        let value = (s_va as i64 + a) as u64;
                        let dest = p_va as *mut u64;
                        core::ptr::write_volatile(dest, value);
                    }
                    2 => {
                        // R_AARCH64_CALL26: 26-bit branch offset replacement (B/BL)
                        let offset = ((s_va as i64 + a - p_va as i64) >> 2) as u32;
                        let dest = p_va as *mut u32;
                        let instr = core::ptr::read_volatile(dest);
                        core::ptr::write_volatile(dest, (instr & 0xFC000000) | (offset & 0x03FFFFFF));
                    }
                    3 => {
                        // R_AARCH64_ADR_PREL_PG_HI21: 21-bit page offset replacement (ADRP)
                        let target_page = (s_va as i64 + a) & !0xFFF;
                        let current_page = p_va as i64 & !0xFFF;
                        let offset = ((target_page - current_page) >> 12) as u32;
                        
                        let dest = p_va as *mut u32;
                        let instr = core::ptr::read_volatile(dest);
                        let immlo = offset & 3;
                        let immhi = (offset >> 2) & 0x7FFFF;
                        core::ptr::write_volatile(dest, (instr & 0x9F00001F) | (immlo << 29) | (immhi << 5));
                    }
                    4 => {
                        // R_AARCH64_ADD_ABS_LO12_NC: 12-bit immediate replacement (ADD)
                        let offset = ((s_va as i64 + a) & 0xFFF) as u32;
                        let dest = p_va as *mut u32;
                        let instr = core::ptr::read_volatile(dest);
                        core::ptr::write_volatile(dest, (instr & 0xFFC003FF) | (offset << 10));
                    }
                    5 => {
                        // R_AARCH64_LDST64_ABS_LO12_NC: LDR/STR 64-bit load/store offset (scaled)
                        let offset = (((s_va as i64 + a) & 0xFFF) >> 3) as u32;
                        let dest = p_va as *mut u32;
                        let instr = core::ptr::read_volatile(dest);
                        core::ptr::write_volatile(dest, (instr & 0xFFC003FF) | (offset << 10));
                    }
                    6 => {
                        // R_AARCH64_LDST32_ABS_LO12_NC: LDR/STR 32-bit load/store offset (scaled)
                        let offset = (((s_va as i64 + a) & 0xFFF) >> 2) as u32;
                        let dest = p_va as *mut u32;
                        let instr = core::ptr::read_volatile(dest);
                        core::ptr::write_volatile(dest, (instr & 0xFFC003FF) | (offset << 10));
                    }
                    _ => {}
                }
            }
        }
    }

    // 5. Invalidate caches to synchronize CPU pipeline
    unsafe {
        // Compute approximate memory span for instruction cache sync
        let total_size = header.file_size as usize;
        <crate::arch::CurrentArch as crate::arch::ArchHardware>::sync_instruction_cache(base_pill_va, total_size);
        crate::arch::CurrentArch::flush_tlb();
    }

    // 6. Spawn and register Pill as an EL1 kernel-mode thread
    let entry_point = base_pill_va + header.entry_point as usize;
    crate::log_info!("PILL-LOADER", "Successfully relocated. Entry point virtual address: {:#x}", entry_point);

    let entry_fn: extern "C" fn() = unsafe { core::mem::transmute(entry_point) };
    let thread = Thread::new_kernel(pill_name, entry_fn)?;

    unsafe {
        crate::task::scheduler::SCHEDULER.add(thread);
    }

    crate::log_info!("PILL-LOADER", "Pill module '{}' fully initialized and enqueued into scheduler!", pill_name);
    Ok(0)
}
