//! GICv2 driver (Generic Interrupt Controller, version 2).
//!
//! QEMU's `virt` machine exposes a GICv2 at fixed MMIO bases that we
//! read from the device tree.  We only enable enough to service one
//! PPI: the generic timer (PPI #30) that we'll wire up in
//! `timer.rs`.
//!
//! ## Register layout
//!
//! - Distributor (`GICD_*`): one register bank at `dist_base`
//! - CPU interface (`GICC_*`): one register bank at `cpu_base`
//!
//! Both register banks are 4 KiB each.
//!
//! ## What we program
//!
//! - `GICD_CTLR`     = 1   (enable group 0)
//! - `GICDist->Enable` for SPIs/PPIs we care about (#30)
//! - `GICC_CTLR`     = 1   (enable group 0)
//! - `GICC_PMR`      = 0xff (lowest priority mask, accept everything)
//!
//! When an IRQ fires the CPU interface latches the highest-priority
//! interrupt ID into `GICC_IAR`.  Our trap stub reads it,
//! dispatches, and writes the same value back to `GICC_EOIR` to
//! mark the IRQ complete.

use core::ptr::{read_volatile, write_volatile};

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER: usize = 0x100;
const GICD_ICENABLER: usize = 0x180;
const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00C;
const GICC_EOIR: usize = 0x010;

static mut GICD_BASE: usize = 0;
static mut GICC_BASE: usize = 0;

#[inline(always)]
unsafe fn gicd(offset: usize) -> *mut u32 {
    (GICD_BASE + offset) as *mut u32
}

#[inline(always)]
unsafe fn gicc(offset: usize) -> *mut u32 {
    (GICC_BASE + offset) as *mut u32
}

/// Install the GIC bases (parsed from the FDT) and enable group 0
/// forwarding for both the distributor and the CPU interface.
pub fn init(dist_base: usize, cpu_base: usize) {
    unsafe {
        GICD_BASE = dist_base;
        GICC_BASE = cpu_base;
    }

    // Disable the distributor while we program it, then re-enable.
    unsafe { write_volatile(gicd(GICD_CTLR), 0) }

    // Enable PPI #30 (the non-secure EL1 physical timer PPI).
    // PPI IDs start at 16, so #30 sits in ISENABLER0, bit 30.
    unsafe {
        write_volatile(gicd(GICD_ISENABLER + 0 * 4), 1u32 << 30);
    }

    // Priority mask: lowest priority, accept everything.
    unsafe { write_volatile(gicc(GICC_PMR), 0xff) }

    // Enable group 0 on both the distributor and the CPU interface.
    unsafe {
        write_volatile(gicd(GICD_CTLR), 1);
        write_volatile(gicc(GICC_CTLR), 1);
    }
    // Diagnostic: read back GICD_CTLR and GICC_CTLR to confirm
    // writes stuck.  If they're 0, the MMIO base is wrong.
    let dctl = unsafe { read_volatile(gicd(GICD_CTLR)) };
    let cctl = unsafe { read_volatile(gicc(GICC_CTLR)) };
    crate::log_info!("GIC", "GICD_CTLR={:#x} GICC_CTLR={:#x}", dctl, cctl);
}

/// Initialize the GIC CPU interface locally for the current CPU core.
pub fn init_local_cpu_interface() {
    unsafe {
        if GICC_BASE != 0 {
            // Priority mask: lowest priority, accept everything.
            write_volatile(gicc(GICC_PMR), 0xff);
            // Enable group 0 on CPU interface.
            write_volatile(gicc(GICC_CTLR), 1);
        }
    }
}

/// Acknowledge an IRQ and return its ID.  Called from the IRQ stub
/// after `daifclr` clears the CPU's I-bit mask.
pub fn ack() -> u32 {
    unsafe { read_volatile(gicc(GICC_IAR)) }
}

/// Signal end-of-interrupt for the given ID.  Must be called after
/// the dispatcher has finished handling the IRQ.
pub fn eoi() {
    // We need to know which ID we're EOI'ing.  The stub reads
    // `IAR` directly into a register; mirror that here.
    let iar = unsafe { read_volatile(gicc(GICC_IAR)) };
    unsafe { write_volatile(gicc(GICC_EOIR), iar) }
}

/// Read the GICD_CTLR register (distributor control).
/// Used for diagnostic prints to confirm GIC state.
pub fn read_gicd_ctlr() -> u32 {
    unsafe { read_volatile(gicd(GICD_CTLR)) }
}

/// Read the pending register for SPIs/PPIs (set bit = enabled).
/// Used for debug prints only.
pub fn is_enabled(intid: u32) -> bool {
    let reg = (intid / 32) as usize;
    let bit = intid % 32;
    unsafe { (read_volatile(gicd(GICD_ISENABLER + reg * 4)) >> bit) & 1 != 0 }
}

/// Trigger a software-generated interrupt (SGI) by writing to the
/// SGIR register.  Format: bits[3:0]=SGIID, bits[15:8]=Aff0,
/// bits[23:16]=CPUTargetList.  Always targets CPU 0 for now.
pub fn trigger_sgi(intid: u32) {
    const GICD_SGIR: usize = 0xF00;
    let value = ((intid & 0xF) << 24) | (1u32 << 16); // SGI ID + CPU 0
    unsafe {
        write_volatile(gicd(GICD_SGIR), value);
    }
}