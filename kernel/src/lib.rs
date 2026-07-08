#![no_std]
#![crate_type = "staticlib"]

extern crate hal;
extern crate shared;

pub mod arch;
pub mod fdt;
pub mod drivers;
pub mod task;
pub mod mm;
pub mod ipc;
pub mod object;
pub mod syscall;
pub mod sync;
pub mod kcore;
pub mod vfs;
pub mod loader;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let pc: usize;
    let sp: usize;
    unsafe {
        #[cfg(target_arch = "aarch64")]
        {
            core::arch::asm!("mov {}, x30", out(reg) pc);
            core::arch::asm!("mov {}, sp", out(reg) sp);
        }
        #[cfg(target_arch = "riscv64")]
        {
            core::arch::asm!("mv {}, ra", out(reg) pc);
            core::arch::asm!("mv {}, sp", out(reg) sp);
        }
    }

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

    #[cfg(target_arch = "aarch64")]
    unsafe {
        let spsr: u64;
        let elr: u64;
        let esr: u64;
        core::arch::asm!("mrs {}, spsr_el1", out(reg) spsr);
        core::arch::asm!("mrs {}, elr_el1", out(reg) elr);
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr);
        crate::kprintln!("=> SPSR_EL1 : {:#018x}", spsr);
        crate::kprintln!("=> ELR_EL1  : {:#018x}", elr);
        crate::kprintln!("=> ESR_EL1  : {:#018x} (Exception Syndrome Register)", esr);
    }

    #[cfg(target_arch = "riscv64")]
    unsafe {
        let sstatus: usize;
        let sepc: usize;
        let scause: usize;
        core::arch::asm!("csrr {}, sstatus", out(reg) sstatus);
        core::arch::asm!("csrr {}, sepc", out(reg) sepc);
        core::arch::asm!("csrr {}, scause", out(reg) scause);
        crate::kprintln!("=> sstatus  : {:#018x}", sstatus);
        crate::kprintln!("=> sepc     : {:#018x}", sepc);
        crate::kprintln!("=> scause   : {:#018x} (Supervisor Cause Register)", scause);
    }

    crate::kprintln!("\x1b[1;31m======================================================================\x1b[0m");
    crate::kprintln!("\x1b[1;31m  SYSTEM HALTED. Please hard reset QEMU.\x1b[0m");
    crate::kprintln!("\x1b[1;31m======================================================================\x1b[0m");

    loop {}
}

pub static mut DTB_POINTER: *const u8 = core::ptr::null();

#[no_mangle]
pub extern "C" fn kernel_main(dtb_ptr: *const u8) {
    unsafe {
        DTB_POINTER = dtb_ptr;
    }

    match fdt::parse(dtb_ptr) {
        Ok(boot) => {
            if boot.uart_type.as_str() == "pl011" {
                drivers::uart::init_pl011(boot.uart_base);
            } else if boot.uart_type.as_str() == "ns16550" {
                drivers::uart::init_ns16550(boot.uart_base);
            }
            arch::early_init();

            crate::kprintln!();
            crate::log_info!("HNX", "Kernel v{}", env!("CARGO_PKG_VERSION"));
            crate::log_info!("FDT", "Discovered hardware:");
            crate::log_info!("FDT", "=> UART base : {:#x}", boot.uart_base);
            crate::log_info!("FDT", "=> RAM base  : {:#x}", boot.ram_base);
            crate::log_info!("FDT", "=> RAM size  : {:#x} ({} MB)", boot.ram_size, boot.ram_size / 1024 / 1024);

            mm::init(boot.ram_base, boot.ram_size);
            let free_cnt = mm::phys::get_free_pages_count();
            let total_cnt = mm::phys::get_total_pages_count();
            crate::log_info!("MM", "Physical page allocator initialized.");
            crate::log_info!("MM", "=> Free pages : {} ({} MB) / {} ({} MB)", free_cnt, free_cnt * 4 / 1024, total_cnt, total_cnt * 4 / 1024);

            arch::mmu::build_and_enable(boot.ram_base, boot.ram_size, boot.uart_base);
            crate::log_info!("MMU", "4-level page tables ACTIVE");

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

            crate::mm::smoke::vmo_vmar_smoke_test();

            // Phase 3.2: bootstrap init process and thread / run smoke tests.
            // if let Err(e) = crate::task::smoke::launch_smoke_tests() {
            //     crate::log_error!("BOOT", "Smoke tests initialization failed: {:?}", e);
            //     loop {}
            // }

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

            // Enable IRQs in S-Mode / EL1 globally so interrupts work
            #[cfg(target_arch = "aarch64")]
            crate::arch::aarch64::trap::enable_irqs();
            #[cfg(target_arch = "riscv64")]
            crate::arch::riscv64::trap::enable_irqs();

            unsafe {
                crate::task::scheduler::SCHEDULER.run();
            }
        }
        Err(e) => {
            #[cfg(target_arch = "riscv64")]
            drivers::uart::init_ns16550(0x10000000);
            #[cfg(not(target_arch = "riscv64"))]
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

            mm::init(ram_base, ram_size);
            let free_cnt = mm::phys::get_free_pages_count();
            let total_cnt = mm::phys::get_total_pages_count();
            crate::log_info!("MM", "Physical page allocator initialized.");
            crate::log_info!("MM", "=> Free pages : {} ({} MB) / {} ({} MB)", free_cnt, free_cnt * 4 / 1024, total_cnt, total_cnt * 4 / 1024);
            
            arch::mmu::build_and_enable(ram_base, ram_size, uart_base);
            crate::log_info!("MMU", "4-level page tables ACTIVE");
            crate::log_info!("BOOT", "OK");

            crate::mm::smoke::vmo_vmar_smoke_test();
        }
    }

    // Phase 3.1: unmask IRQs and idle.  The generic timer is
    // programmed for one tick every TIMER_INTERVAL_TICKS counter
    // cycles; on each tick the trap stub calls `drivers::timer::
    // handle_tick` which prints "[tick N @ 0x...]" to the UART.
    #[cfg(target_arch = "aarch64")]
    {
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

        // The timer was already initialized in `drivers::timer::init()`
        // above (TVAL=1M, CTL=ENABLE).  It should fire every ~16 ms.
        // Enable IRQs and enter WFI loop.
        arch::aarch64::trap::enable_irqs();
        loop {
            unsafe { core::arch::asm!("wfi", options(nomem, nostack)) }
        }
    }
    #[cfg(target_arch = "riscv64")]
    {
        crate::log_info!("IDLE", "unmasking IRQs and waiting for timer ticks");
        let stvec: u64;
        let sstatus: u64;
        let sie: u64;
        unsafe {
            core::arch::asm!(
                "csrr {0}, stvec",
                "csrr {1}, sstatus",
                "csrr {2}, sie",
                out(reg) stvec, out(reg) sstatus, out(reg) sie,
                options(nomem, preserves_flags),
            );
        }
        crate::log_info!("IDLE", "stvec={:#x} sstatus={:#x} sie={:#x}", stvec, sstatus, sie);

        arch::riscv64::trap::enable_irqs();
        loop {
            unsafe { core::arch::asm!("wfi", options(nomem, nostack)) }
        }
    }
}
