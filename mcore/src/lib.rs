#![no_std]
#![crate_type = "staticlib"]

extern crate alloc;
extern crate shared;

pub mod arch;
pub mod fdt;
pub mod drivers;
pub mod task;
pub mod memory;
pub mod ipc;
pub mod object;
pub mod syscall;
pub mod sync;
pub mod kcore;
pub mod vfs;
pub mod loader;
pub mod rootfs;
pub mod smp;
pub mod pillsmod;

use crate::arch::ArchHardware;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let (pc, sp) = crate::arch::CurrentArch::get_current_registers();

    crate::kprintln!("\n\x1b[1;31m======================================================================\x1b[0m");
    crate::kprintln!("\x1b[1;31m  🔥 KERNEL PANIC: Supervisor Exception 🔥\x1b[0m");
    crate::kprintln!("\x1b[1;31m======================================================================\x1b[0m");

    crate::kprintln!("=> \x1b[1mReason\x1b[0m   : {}", info.message());

    if let Some(loc) = info.location() {
        crate::kprintln!("=> \x1b[1mLocation\x1b[0m : {}:{}", loc.file(), loc.line());
    }

    crate::kprintln!("=> \x1b[1mPC (LR)\x1b[0m  : {:#018x}", pc);
    crate::kprintln!("=> \x1b[1mSP\x1b[0m       : {:#018x}", sp);
    crate::kprintln!("\x1b[90m----------------------------------------------------------------------\x1b[0m");
    crate::kprintln!("=> \x1b[1m[ ARCH-SPECIFIC REGISTERS ]\x1b[0m");

    let diag = crate::arch::CurrentArch::get_diagnostics();
    crate::kprintln!("=> STATUS/SPSR : {:#018x}", diag.spsr_or_status);
    crate::kprintln!("=> EPC/ELR     : {:#018x}", diag.elr_or_epc);
    crate::kprintln!("=> CAUSE/ESR   : {:#018x}", diag.esr_or_cause);

    crate::kprintln!("\x1b[1;31m======================================================================\x1b[0m");
    crate::kprintln!("\x1b[1;31m  SYSTEM HALTED. Please hard reset QEMU.\x1b[0m");
    crate::kprintln!("\x1b[1;31m======================================================================\x1b[0m");

    loop {}
}

pub static mut DTB_POINTER: *const u8 = core::ptr::null();
pub static mut BOOTFS_PHYS_ADDR: usize = 0;
pub static mut BOOTFS_PHYS_SIZE: usize = 0;
pub static mut SERVICES_PHYS_ADDR: usize = 0x48000000;
pub static mut SERVICES_PHYS_SIZE: usize = 8 * 1024 * 1024;

static mut DEVICE_INFO_BOOT: core::mem::MaybeUninit<crate::fdt::BootInfo> =
    core::mem::MaybeUninit::new(crate::fdt::BootInfo::empty());
static mut DEVICE_INFO_VALID: bool = false;

#[no_mangle]
pub extern "C" fn kernel_main(dtb_ptr: *const u8, bootfs_pa: usize, bootfs_size: usize, pill_pa: usize, pill_size: usize) {
    unsafe {
        DTB_POINTER = dtb_ptr;
        BOOTFS_PHYS_ADDR = bootfs_pa;
        BOOTFS_PHYS_SIZE = bootfs_size;
    }

    // Slot 0 is the primary CPU.  Record it before any SMP code
    // path expects `current_core_id()` to return a known value.
    smp::register_core(0);

    crate::log_info!("BOOT", "kernel_main entry: dtb={:?}, bootfs={:#x}/{}, pill={:#x}/{}", dtb_ptr, bootfs_pa, bootfs_size, pill_pa, pill_size);

    match fdt::parse(dtb_ptr) {
        Ok(boot) => {
            // Dynamically update the early UART base address from FDT parsing.
            drivers::uart::EARLY_UART_BASE.store(boot.uart_base, core::sync::atomic::Ordering::SeqCst);

            unsafe {
                DEVICE_INFO_BOOT.write(boot);
                DEVICE_INFO_VALID = true;
            }

            let boot = unsafe { &*DEVICE_INFO_BOOT.as_ptr() };

            if boot.uart_type.as_str() == "pl011" {
                drivers::uart::init_pl011(boot.uart_base);
            } else if boot.uart_type.as_str() == "ns16550" {
                drivers::uart::init_ns16550(boot.uart_base);
            }
            arch::early_init();

            crate::kprintln!();
            crate::log_info!("HNX", "{}", option_env!("CAPSULEOS_VERSION").unwrap_or("capsuleOS Pangu v1.0.0 (dev)"));
            crate::log_info!("FDT", "Discovered hardware:");
            crate::log_info!("FDT", "=> UART base : {:#x}", boot.uart_base);
            crate::log_info!("FDT", "=> RAM base  : {:#x}", boot.ram_base);
            crate::log_info!("FDT", "=> RAM size  : {:#x} ({} MB)", boot.ram_size, boot.ram_size / 1024 / 1024);

            crate::arch::phys::init(boot.ram_base, boot.ram_size);
            let free_cnt = crate::arch::aarch64::phys::get_free_pages_count();
            let total_cnt = crate::arch::aarch64::phys::get_total_pages_count();
            crate::log_info!("MM", "Physical page allocator initialized.");
            crate::log_info!("MM", "=> Free pages : {} ({} MB) / {} ({} MB)", free_cnt, free_cnt * 4 / 1024, total_cnt, total_cnt * 4 / 1024);

            arch::mmu::build_and_enable(boot.ram_base, boot.ram_size, boot.uart_base);
            crate::log_info!("MMU", "4-level page tables ACTIVE");

            // Dynamically detect Services VFS size, falling back to BOOTFS if not found
            unsafe {
                if let Some(detected_size) = detect_vfs_size(SERVICES_PHYS_ADDR) {
                    SERVICES_PHYS_SIZE = detected_size;
                } else {
                    crate::log_info!("BOOT", "No separate services.img found. Sharing BootFS at physical {:#x}!", BOOTFS_PHYS_ADDR);
                    SERVICES_PHYS_ADDR = BOOTFS_PHYS_ADDR;
                    SERVICES_PHYS_SIZE = BOOTFS_PHYS_SIZE;
                }
            }

            // Phase 3.1: bring up the GIC and the generic timer.
            // This must happen AFTER the MMU is on so the timer reads
            // (which use the virtual system-register interface) and
            // the GIC MMIO accesses go through our page-table walker.
            #[cfg(target_arch = "aarch64")]
            {
                if boot.gicd_base != 0 && boot.gicc_base != 0 {
                    crate::log_info!("IRQ", "GICD={:#x} GICC={:#x}", boot.gicd_base, boot.gicc_base);
                    drivers::gic::init(boot.gicd_base, boot.gicc_base);
                    drivers::timer::init();
                    crate::log_info!("IRQ", "GIC + generic timer enabled");
                } else {
                    crate::log_warn!("IRQ", "no GIC in FDT, skipping timer bring-up");
                }
            }
            #[cfg(target_arch = "riscv64")]
            {
                drivers::timer::init();
                crate::log_info!("IRQ", "RISC-V Supervisor timer enabled");
            }

            crate::log_info!("BOOT", "OK");

            crate::memory::smoke::vmo_vmar_smoke_test();

            if pill_pa != 0 {
                let _ = crate::pillsmod::load_all_from_bootloader(pill_pa);
            }

            match crate::loader::launch_loader() {
                Ok(_) => {
                    crate::log_info!("BOOT", "Loader process ready to schedule");
                }
                Err(e) => {
                    crate::log_warn!("BOOT", "Loader skipped or failed ({:?}), falling back to kernel smoke threads", e);
                }
            }

            // Start preemptive scheduling!
            crate::log_info!("SCHED", "Starting preemptive multitasking...");

            // Probe DTB + PSCI; record topology.  Cheap & idempotent.
            smp::probe::probe_cpus();
            smp::boot::log_topology();

            // Boot secondary multi-core CPUs (PSCI CPU_ON)
            smp::boot::boot_secondary_cores();

            // Enable IRQs globally so interrupts work
            crate::arch::aarch64::trap::enable_irqs();

            unsafe {
                crate::task::scheduler::SCHEDULER.run();
            }
        }
        Err(e) => {
            drivers::uart::init_pl011(0x09000000);

            arch::early_init();
            crate::kprintln!();
            crate::log_info!("HNX", "Kernel v{}", env!("CARGO_PKG_VERSION"));
            crate::log_error!("FDT", "FDT parse failed: {}", e);
            crate::log_warn!("FDT", "Using default architecture UART fallback");

            #[cfg(target_arch = "riscv64")]
            let (ram_base, ram_size, uart_base) = (0x80000000usize, 512 * 1024 * 1024, 0x10000000usize);
            #[cfg(not(target_arch = "riscv64"))]
            let (ram_base, ram_size, uart_base) = (0x40000000usize, 512 * 1024 * 1024, 0x09000000usize);

            crate::arch::phys::init(ram_base, ram_size);
            let free_cnt = crate::arch::aarch64::phys::get_free_pages_count();
            let total_cnt = crate::arch::aarch64::phys::get_total_pages_count();
            crate::log_info!("MM", "Physical page allocator initialized.");
            crate::log_info!("MM", "=> Free pages : {} ({} MB) / {} ({} MB)", free_cnt, free_cnt * 4 / 1024, total_cnt, total_cnt * 4 / 1024);
            
            arch::mmu::build_and_enable(ram_base, ram_size, uart_base);
            crate::log_info!("MMU", "4-level page tables ACTIVE");
            crate::log_info!("BOOT", "OK");

            crate::memory::smoke::vmo_vmar_smoke_test();
        }
    }

    crate::log_info!("IDLE", "unmasking IRQs and waiting for timer ticks");
    let vbar: u64;
    let daif: u64;
    let ctl: u64;
    unsafe {
        core::arch::asm!(
            "mrs {0}, vbar_el1",
            "mrs {1}, daif",
            "mrs {2}, cntp_ctl_el0",
            out(reg) vbar, out(reg) daif, out(reg) ctl,
            options(nomem, preserves_flags),
        );
    }
    crate::log_info!("IDLE", "vbar={:#x} daif={:#x} cntp_ctl={:#x}", vbar, daif, ctl);

    arch::aarch64::trap::enable_irqs();
    loop {
        unsafe { crate::arch::CurrentArch::wait_for_interrupt() }
    }
}

fn detect_vfs_size(phys_addr: usize) -> Option<usize> {
    unsafe {
        let base = crate::arch::mmu_facade::pa_to_kernel_va(phys_addr) as *const u8;
        let mut sig = [0u8; 8];
        core::ptr::copy_nonoverlapping(base, sig.as_mut_ptr(), 8);
        if &sig == b"HNXF_VFS" {
            let mut count_bytes = [0u8; 8];
            core::ptr::copy_nonoverlapping(base.add(8), count_bytes.as_mut_ptr(), 8);
            let count = u64::from_le_bytes(count_bytes) as usize;

            let mut max_end = 16;
            for i in 0..count {
                let entry_offset = 16 + i * 144;
                let path_end = entry_offset + 128;

                let mut off_bytes = [0u8; 8];
                core::ptr::copy_nonoverlapping(base.add(path_end), off_bytes.as_mut_ptr(), 8);
                let file_offset = u64::from_le_bytes(off_bytes) as usize;

                let mut sz_bytes = [0u8; 8];
                core::ptr::copy_nonoverlapping(base.add(path_end + 8), sz_bytes.as_mut_ptr(), 8);
                let file_size = u64::from_le_bytes(sz_bytes) as usize;

                let end = file_offset + file_size;
                if end > max_end {
                    max_end = end;
                }
            }
            Some((max_end + 4095) & !4095)
        } else {
            None
        }
    }
}
