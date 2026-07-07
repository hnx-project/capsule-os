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
pub static mut TEST_CHANNEL: Option<crate::ipc::Channel> = None;
pub static mut TEST_PORT: Option<crate::ipc::port::Port> = None;

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
            crate::log_info!("KERNEL", "HNX v{}", env!("CARGO_PKG_VERSION"));
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

            vmo_vmar_smoke_test();

            // Phase 3.2: bootstrap init process and thread.
            // Phase 3.3: register handle table and run handle smoke test.
            {
                use crate::task::{Process, Thread};
                use crate::syscall::set_handle_table;

                unsafe {
                    TEST_CHANNEL = Some(crate::ipc::Channel::new().unwrap());
                    TEST_PORT = Some(crate::ipc::port::Port::new(8).unwrap());
                }
                
                extern "C" fn init_entry() {
                    // Create a Vmo via syscall
                    let vmo_handle_raw = crate::syscall::syscall_dispatch(
                        crate::syscall::numbers::SYSCALL_VMO_CREATE, 16 * 1024, 0, 0, 0, 0, 0
                    );
                    crate::log_info!("TEST_A", "init_entry: Created VMO via syscall -> handle = {}", vmo_handle_raw);
                    
                    // Write data into Vmo via syscall
                    let pattern = b"VMO_TRANSFER_OK";
                    let written = crate::syscall::syscall_dispatch(
                        crate::syscall::numbers::SYSCALL_VMO_WRITE, vmo_handle_raw, 0, pattern.as_ptr() as usize, pattern.len(), 0, 0
                    );
                    crate::log_info!("TEST_A", "init_entry: Wrote '{}' to VMO", core::str::from_utf8(pattern).unwrap());

                    // Write to channel and transfer the VMO handle!
                    crate::log_info!("TEST_A", "init_entry: Writing to channel and transferring VMO handle...");
                    let msg = b"HELLO_VMO_TRANS";
                    let handles_to_send = [shared::types::HandleValue::new(vmo_handle_raw as u32)];
                    let bytes_written = unsafe { TEST_CHANNEL.as_mut().unwrap().write(msg, &handles_to_send) }.unwrap_or(0);
                    crate::log_info!("TEST_A", "init_entry: Woken up! Bytes written = {}", bytes_written);

                    // Verify VMO handle is revoked
                    let mut dummy_buf = [0u8; 16];
                    let test_res = crate::syscall::syscall_dispatch(
                        crate::syscall::numbers::SYSCALL_VMO_READ, vmo_handle_raw, 0, dummy_buf.as_mut_ptr() as usize, 16, 0, 0
                    );
                    if test_res == shared::status::Status::BadHandle.to_raw() {
                        crate::log_info!("TEST_A", "init_entry: Revocation proof PASS! VMO handle is no longer accessible by init!");
                    } else {
                        crate::log_error!("TEST_A", "init_entry: Revocation proof FAIL! VMO handle is still accessible!");
                    }

                    // --- Phase 4.3 Async Completion Port test ---
                    crate::log_info!("TEST_A", "init_entry: Waiting for events on Port (will block)...");
                    let packet = unsafe { TEST_PORT.as_mut().unwrap().wait() }.unwrap();
                    crate::log_info!("TEST_A", "init_entry: Port event received! key={:#x}, trigger={}", packet.key, packet.trigger);

                    loop {
                        // Wait for preemption or sleep
                    }
                }
                
                extern "C" fn worker_entry() {
                    crate::log_info!("TEST_B", "worker_entry: Reading from channel (will wake up init and receive VMO)...");
                    let mut msg_buf = [0u8; 32];
                    let mut recv_handles = [shared::types::HandleValue::new(0); 2];
                    let bytes_read = unsafe { TEST_CHANNEL.as_mut().unwrap().read(&mut msg_buf, &mut recv_handles) }.unwrap_or(0);
                    crate::log_info!("TEST_B", "worker_entry: Read successful! Msg = '{}'", core::str::from_utf8(&msg_buf[..bytes_read]).unwrap_or("?"));

                    // Access the transferred VMO!
                    let received_vmo_handle = recv_handles[0].get();
                    crate::log_info!("TEST_B", "worker_entry: Received transferred VMO! New handle assigned to worker = {}", received_vmo_handle);

                    let mut vmo_buf = [0u8; 16];
                    let read_res = crate::syscall::syscall_dispatch(
                        crate::syscall::numbers::SYSCALL_VMO_READ, received_vmo_handle as usize, 0, vmo_buf.as_mut_ptr() as usize, 16, 0, 0
                    );
                    let vmo_str = core::str::from_utf8(&vmo_buf[..read_res]).unwrap_or("?");
                    crate::log_info!("TEST_B", "worker_entry: Read from transferred VMO successful! Content = '{}'", vmo_str);

                    // --- Phase 4.3 Async Completion Port test ---
                    crate::log_info!("TEST_B", "worker_entry: Queueing an asynchronous PortPacket event...");
                    let mut packet = crate::ipc::port::PortPacket::default();
                    packet.key = 0x1234_5678;
                    packet.trigger = 42;
                    unsafe { TEST_PORT.as_mut().unwrap().queue(&packet) }.unwrap();

                    loop {
                        // Wait for preemption or sleep
                    }
                }
                
                match Process::new("init") {
                    Ok(proc_init) => {
                        match Process::new("worker") {
                            Ok(proc_worker) => {
                                match Thread::new_kernel("init", init_entry) {
                                    Ok(mut init_thread) => {
                                        match Thread::new_kernel("worker", worker_entry) {
                                            Ok(mut worker_thread) => {
                                                // Bind each thread to its own Process's HandleTable for true isolation!
                                                init_thread.process_id = proc_init.id;
                                                init_thread.handle_table = &proc_init.handle_table;

                                                worker_thread.process_id = proc_worker.id;
                                                worker_thread.handle_table = &proc_worker.handle_table;

                                                crate::log_info!("TASK", "init & worker threads created (with isolated process handle tables)");
                                                
                                                // Set state to Ready so they can be scheduled
                                                init_thread.state = crate::task::thread::ThreadState::Ready;
                                                worker_thread.state = crate::task::thread::ThreadState::Ready;
                                                
                                                // Try launching the user-space loader
                                                match launch_loader() {
                                                    Ok(_) => {
                                                        crate::log_info!("BOOT", "Loader process ready to schedule");
                                                    }
                                                    Err(e) => {
                                                        crate::log_warn!("BOOT", "Loader skipped or failed ({:?}), falling back to kernel smoke threads", e);
                                                        unsafe {
                                                            crate::task::scheduler::SCHEDULER.add(init_thread);
                                                            crate::task::scheduler::SCHEDULER.add(worker_thread);
                                                        }
                                                    }
                                                }
                                                
                                                // Phase 3.3: wire syscall dispatch to the init process by default (for handle_smoke_test).
                                                set_handle_table(&proc_init.handle_table);
                                                crate::log_info!("HANDLE", "handle table registered");
                                                handle_smoke_test();
                                                
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
                                                crate::log_error!("TASK", "worker thread creation failed: {:?}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        crate::log_error!("TASK", "init thread creation failed: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                crate::log_error!("TASK", "worker process creation failed: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        crate::log_error!("TASK", "init process::new failed: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            #[cfg(target_arch = "riscv64")]
            drivers::uart::init_ns16550(0x10000000);
            #[cfg(not(target_arch = "riscv64"))]
            drivers::uart::init_pl011(0x09000000);

            arch::early_init();
            crate::kprintln!();
            crate::log_info!("KERNEL", "HNX v{}", env!("CARGO_PKG_VERSION"));
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

            vmo_vmar_smoke_test();
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

/// Phase 2.4 smoke test: build a VMO, allocate pages for it, write
/// data into it, then map it into a freshly-allocated VMAR.  We then
/// re-read through the VMAR's virtual address and confirm we see
/// what was written.
///
/// On AArch64 the MMU is on and the high-half kernel offset applies
/// to all kernel-VA reads.  We map the VMO into a *user* VA range
/// (low half) and read from there so the test exercises the
/// TTBR0 / 4 KiB page mapping path.  The kernel can still read
/// those VAs because TTBR0 also identity-maps the low half
/// (`0x0000_0000_0000_0000` etc. via the existing L0[0] -> L1_ID
/// table).
///
/// On RISC-V the MMU is currently off in our build, so `map_page`
/// only writes the page table; reads through the virtual address
/// still go through the identity mapping.  The VMO side of the
/// test (`write` + `read` from kernel VA) is what really exercises
/// the new code there.
fn vmo_vmar_smoke_test() {
    use crate::mm::vmar::{Vmar, VmarFlags};
    use crate::mm::vmo::Vmo;

    crate::log_info!("SMOKE", "VMO/VMAR smoke test");

    // 1) Create a 16 KiB VMO (4 pages) and commit it.
    let mut vmo = match Vmo::create_with_size(16 * 1024) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error!("SMOKE", "create_with_size failed: {:?}", e);
            return;
        }
    };
    if let Err(e) = vmo.commit_all() {
        crate::log_error!("SMOKE", "commit_all failed: {:?}", e);
        return;
    }

    // 2) Write a recognizable pattern.
    let pattern = b"VMO_VMAR_OK";
    if let Err(e) = vmo.write(0, pattern) {
        crate::log_error!("SMOKE", "vmo.write failed: {:?}", e);
        return;
    }

    // 3) Read it back from the VMO.
    let mut back = [0u8; 12];
    let n = match vmo.read(0, &mut back) {
        Ok(n) => n,
        Err(e) => {
            crate::log_error!("SMOKE", "vmo.read failed: {:?}", e);
            return;
        }
    };

    crate::log_info!("SMOKE", "=> vmo roundtrip: {} bytes, got: {}", n, core::str::from_utf8(&back[..n]).unwrap_or("?"));

    // 4) Carve a VMAR and map the VMO into it.  Choose a VA in the
    // low half on AArch64 (TTBR0) and in identity-mapped territory
    // on RISC-V.
    #[cfg(target_arch = "aarch64")]
    let vmar_base = 0x0000_0010_0000usize;  // 1 MiB, in the user range
    #[cfg(target_arch = "riscv64")]
    let vmar_base = 0x9000_0000usize;       // identity-mapped, free of UART

    let mut root = match Vmar::create(vmar_base, 0x0010_0000) {
        Ok(v) => v,
        Err(e) => {
            crate::log_error!("SMOKE", "Vmar::create failed: {:?}", e);
            return;
        }
    };

    crate::log_info!("SMOKE", "=> root vmar @ {:#x} + {:#x}", root.base, root.size);

    if let Err(e) = root.map(&mut vmo, 0, vmar_base, 16 * 1024,
                              VmarFlags::from_bits(VmarFlags::READ.bits() | VmarFlags::WRITE.bits())) {
        crate::log_error!("SMOKE", "vmar.map failed: {:?}", e);
        return;
    }
    crate::log_info!("SMOKE", "=> vmar.map: 16 KiB VMO -> user VA range OK");

    // 5) Read the pattern back from the virtual address and compare.
    #[cfg(target_arch = "aarch64")]
    {
        let va_ptr = vmar_base as *const u8;
        let mut via_va = [0u8; 12];
        unsafe {
            core::ptr::copy_nonoverlapping(va_ptr, via_va.as_mut_ptr(), 12);
        }
        crate::log_info!("SMOKE", "=> via VA     : {} bytes, got: {}", 12, core::str::from_utf8(&via_va).unwrap_or("?"));

        if &via_va[..pattern.len()] == pattern {
            crate::log_info!("SMOKE", "=> result     : MATCH (via MMU translation)");
        } else {
            crate::log_error!("SMOKE", "=> result     : MISMATCH");
        }
    }
    #[cfg(target_arch = "riscv64")]
    {
        if let Some(pa) = vmo.get_page_phys(0) {
            let kv = crate::arch::mmu::pa_to_kernel_va(pa.as_usize()) as *const u8;
            let mut via_pa = [0u8; 12];
            unsafe {
                core::ptr::copy_nonoverlapping(kv, via_pa.as_mut_ptr(), 12);
            }
            crate::log_info!("SMOKE", "=> via PA     : {} bytes, got: {}", 12, core::str::from_utf8(&via_pa).unwrap_or("?"));
            if &via_pa[..pattern.len()] == pattern {
                crate::log_info!("SMOKE", "=> result     : MATCH (via VMO PA, MMU off)");
            } else {
                crate::log_error!("SMOKE", "=> result     : MISMATCH");
            }
        }
    }

    // 6) Unmap and re-read
    if let Err(e) = root.unmap(vmar_base, 16 * 1024) {
        crate::log_error!("SMOKE", "vmar.unmap failed: {:?}", e);
    } else {
        crate::log_info!("SMOKE", "=> vmar.unmap: OK");
    }

    crate::log_info!("SMOKE", "VMO/VMAR done");
}

/// Phase 3.3: exercise the handle table through syscall dispatch.
fn handle_smoke_test() {
    use crate::syscall::numbers::*;
    use shared::status::Status;

    // 1. Create a VMO via the handle table.
    let hv = crate::syscall::syscall_dispatch(SYSCALL_VMO_CREATE, 16 * 1024, 0, 0, 0, 0, 0);
    if hv == Status::Ok.to_raw() {
        crate::log_error!("HANDLE", "vmo_create: FAILED (returned Ok)");
        return;
    }
    crate::log_info!("HANDLE", "vmo_create -> handle={}", hv);

    // 2. Write data via the handle table.
    let pattern = b"HANDLE_OK";
    let written = crate::syscall::syscall_dispatch(
        SYSCALL_VMO_WRITE, hv, 0, pattern.as_ptr() as usize, pattern.len(), 0, 0,
    );
    if (written as isize) < 0 {
        crate::log_error!("HANDLE", "sys_vmo_write failed: {:?}", Status::from_raw(written as i32));
        return;
    }
    crate::log_info!("HANDLE", "=> sys_vmo_write: {} bytes", written);

    // 3. Read it back.
    let mut buf = [0u8; 16];
    let read_n = crate::syscall::syscall_dispatch(
        SYSCALL_VMO_READ, hv, 0, buf.as_mut_ptr() as usize, buf.len(), 0, 0,
    );
    if (read_n as isize) < 0 {
        crate::log_error!("HANDLE", "sys_vmo_read failed: {:?}", Status::from_raw(read_n as i32));
        return;
    }
    let read_slice = &buf[..core::cmp::min(read_n, buf.len())];
    crate::log_info!("HANDLE", "=> sys_vmo_read: {} bytes, got: {}", read_n, core::str::from_utf8(read_slice).unwrap_or("?"));

    if &buf[..pattern.len()] == pattern {
        crate::log_info!("HANDLE", "smoke test PASS");
    } else {
        crate::log_error!("HANDLE", "smoke test FAIL (data mismatch)");
    }
}

#[inline(never)]
pub(crate) fn print(s: &str) {
    for c in s.bytes() {
        arch::console_putchar(c);
    }
}

pub fn launch_loader() -> shared::status::Result<()> {
    use shared::status::Status;
    use crate::mm::vmo::Vmo;
    use crate::task::thread::Thread;

    let bytes = include_bytes!("../files/init");
    if bytes.len() < 32 {
        return Err(Status::InvalidArgs);
    }

    let magic = &bytes[0..4];
    if magic != b"OHLK" {
        return Err(Status::InvalidArgs);
    }

    let entry = u64::from_le_bytes([
        bytes[6], bytes[7], bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13]
    ]) as usize;
    let segment_count = u16::from_le_bytes([bytes[14], bytes[15]]) as usize;

    crate::log_info!("LOADER", "Embedding loader: entry={:#x}, segments={}", entry, segment_count);

    let proc = crate::task::process::allocate_process("loader")?;
    let pid = proc.id;

    let mut offset = 32;
    let payload_start = 32 + segment_count * 24;

    for i in 0..segment_count {
        if offset + 24 > bytes.len() {
            crate::log_error!("LOADER", "Segment {} descriptor out of bytes", i);
            return Err(Status::InvalidArgs);
        }
        let virt_addr = u64::from_le_bytes([
            bytes[offset], bytes[offset+1], bytes[offset+2], bytes[offset+3],
            bytes[offset+4], bytes[offset+5], bytes[offset+6], bytes[offset+7]
        ]) as usize;
        let file_offset = u64::from_le_bytes([
            bytes[offset+8], bytes[offset+9], bytes[offset+10], bytes[offset+11],
            bytes[offset+12], bytes[offset+13], bytes[offset+14], bytes[offset+15]
        ]) as usize;
        let size = u32::from_le_bytes([
            bytes[offset+16], bytes[offset+17], bytes[offset+18], bytes[offset+19]
        ]) as usize;
        let flags_raw = u32::from_le_bytes([
            bytes[offset+20], bytes[offset+21], bytes[offset+22], bytes[offset+23]
        ]);

        offset += 24;

        let aligned_vaddr = virt_addr & !(4096 - 1);
        let alignment_offset = virt_addr - aligned_vaddr;
        let aligned_size = (size + alignment_offset + 4095) & !(4095);

        let target_va = proc.root_vmar.base + aligned_vaddr;
        let flags = crate::mm::vmar::VmarFlags::from_bits(flags_raw);

        crate::log_info!(
            "LOADER",
            "Segment {}: virt_addr={:#x} (aligned={:#x}, offset={}), size={} (aligned={}), flags={:#x}, vmar_base={:#x}",
            i, virt_addr, aligned_vaddr, alignment_offset, size, aligned_size, flags_raw, proc.root_vmar.base
        );

        let mut vmo = Vmo::create_with_size(aligned_size)?;
        let segment_payload = &bytes[payload_start + file_offset .. payload_start + file_offset + size];
        vmo.write(alignment_offset, segment_payload)?;
        
        match proc.root_vmar.map(&mut vmo, 0, target_va, aligned_size, flags) {
            Ok(_) => {
                crate::log_info!("LOADER", "Mapped segment {}: VA={:#x}, size={}", i, target_va, aligned_size);
            }
            Err(e) => {
                crate::log_error!("LOADER", "Failed to map segment {}: target_va={:#x}, size={}, err={:?}", i, target_va, aligned_size, e);
                return Err(e);
            }
        }
    }

    let stack_size = 16 * 1024;
    let mut stack_vmo = Vmo::create_with_size(stack_size)?;
    let stack_vaddr_offset = 0x2000000;
    let stack_va = proc.root_vmar.base + stack_vaddr_offset;
    
    let stack_flags = crate::mm::vmar::VmarFlags::from_bits(
        crate::mm::vmar::VmarFlags::READ.bits() | 
        crate::mm::vmar::VmarFlags::WRITE.bits() | 
        crate::mm::vmar::VmarFlags::USER.bits()
    );

    proc.root_vmar.map(&mut stack_vmo, 0, stack_va, stack_size, stack_flags)?;
    let stack_top = stack_va + stack_size;

    let loader_entry = proc.root_vmar.base + entry;
    let mut thread = Thread::new_user("loader", loader_entry, stack_top)?;
    thread.process_id = pid;
    thread.handle_table = &proc.handle_table;
    thread.state = crate::task::thread::ThreadState::Ready;

    unsafe {
        crate::task::scheduler::SCHEDULER.add(thread);
    }

    crate::log_info!("LOADER", "Loader service launched successfully at EL0!");
    Ok(())
}
