#![no_std]
#![no_main]

mod arch;
mod platform;

use fdt::Fdt;

extern "C" {
    fn _start();
}

#[no_mangle]
extern "C" fn rust_main(dtb_ptr: *const u8) -> ! {
    platform::init();

    // 1. Parse DTB early to find UART base address (self-adaptive PL011 MMIO)
    let dtb_to_use = if dtb_ptr.is_null() {
        let candidate = platform::DTB_FALLBACK_ADDR as *const u8;
        if let Ok(_fdt) = unsafe { Fdt::from_ptr(candidate) } {
            candidate
        } else {
            core::ptr::null()
        }
    } else {
        dtb_ptr
    };

    let mut parsed_uart_base: Option<usize> = None;
    if !dtb_to_use.is_null() {
        if let Ok(fdt) = unsafe { Fdt::from_ptr(dtb_to_use) } {
            // Check known paths directly to avoid complex tree parsing API mismatches
            if fdt.find_node("/soc/serial@7e201000").is_some() {
                parsed_uart_base = Some(0x3f20_1000); // RPi 3 / Zero 2 W physical view
            } else if fdt.find_node("/soc/serial@fe201000").is_some() {
                parsed_uart_base = Some(0xfe20_1000); // RPi 4 physical view
            } else if fdt.find_node("/pl011@9000000").is_some() {
                parsed_uart_base = Some(0x0900_0000); // QEMU virt
            }
        }
    }

    if let Some(base) = parsed_uart_base {
        unsafe {
            platform::UART_BASE_ADDR = base;
        }
    }

    // Now we can output early logs using the correct UART base!
    log_info!("BOOT", "Booting v{}...", env!("CARGO_PKG_VERSION"));

    if !dtb_to_use.is_null() {
        log_info!(
            "BOOT",
            "DTB parsed successfully at {:#x}.",
            dtb_to_use as usize
        );
        if let Some(base) = parsed_uart_base {
            log_info!("BOOT", "=> UART     : PL011 Base Address = {:#x}", base);
        }
    } else {
        log_warn!("BOOT", "DTB not found.");
    }

    // 2. Self-Adaptive OHC_BASE and BOOTFS_BASE Detection
    let bootloader_load_addr = _start as usize;
    let potential_ohc_base = bootloader_load_addr + 131072;

    let header_bytes = unsafe {
        core::slice::from_raw_parts(potential_ohc_base as *const u8, ohlink_format::OHLK_Header::SIZE)
    };

    let mut ohc_base_addr = platform::DEFAULT_OHC_BASE;
    let mut bootfs_pa = platform::DEFAULT_BOOTFS_BASE;
    let mut dynamic_mode = false;

    if let Ok(header) = ohlink_format::OHLK_Header::from_bytes(header_bytes) {
        // Valid OHLINK header found right after the bootloader! (RPi/Unified boot)
        ohc_base_addr = potential_ohc_base;
        dynamic_mode = true;

        let kernel_size = header.file_size as usize;
        let aligned_kernel_size = (kernel_size + 4095) & !4095;
        let potential_bootfs_base = ohc_base_addr + aligned_kernel_size;

        // Verify if a valid HNXF_VFS image is placed right after the kernel
        let sig_bytes = unsafe { core::slice::from_raw_parts(potential_bootfs_base as *const u8, 8) };
        if sig_bytes == b"HNXF_VFS" {
            bootfs_pa = potential_bootfs_base;
        } else {
            // Check if initramfs was loaded independently (e.g. by RPi GPU at 0x46000000)
            let rpi_initramfs_base = 0x4600_0000;
            let rpi_sig_bytes = unsafe { core::slice::from_raw_parts(rpi_initramfs_base as *const u8, 8) };
            if rpi_sig_bytes == b"HNXF_VFS" {
                bootfs_pa = rpi_initramfs_base;
            }
        }
    }

    let ohc_base = ohc_base_addr as *const u8;
    log_info!(
        "BOOT",
        "Loading Kernel from {} Address: {:#x}",
        if dynamic_mode { "DYNAMIC" } else { "STATIC" },
        ohc_base_addr
    );

    let header_bytes =
        unsafe { core::slice::from_raw_parts(ohc_base, ohlink_format::OHLK_Header::SIZE) };

    let header = match ohlink_format::OHLK_Header::from_bytes(header_bytes) {
        Ok(h) => h,
        Err(e) => {
            log_error!("BOOT", "Failed to parse OHLINK header: {:?}", e);
            arch::halt();
        }
    };

    let entry_point = header.entry_point;

    log_info!("BOOT", "Valid OHLINK Image Found!");
    log_info!(
        "BOOT",
        "=> Version : {}.{}",
        header.version_major,
        header.version_minor
    );
    log_info!("BOOT", "=> Entry   : {:#018x}", entry_point);
    log_info!("BOOT", "=> Segments: {}", header.header_count);

    if header.header_count == 0 {
        log_error!("BOOT", "No segments in OHLINK image!");
        arch::halt();
    }

    let entry_offset = header.header_offset as usize;
    let entry_bytes = unsafe {
        core::slice::from_raw_parts(ohc_base.add(entry_offset), ohlink_format::OHLK_Entry::SIZE)
    };

    let entry = match ohlink_format::OHLK_Entry::from_bytes(entry_bytes) {
        Ok(e) => e,
        Err(e) => {
            log_error!("BOOT", "Failed to parse OHLINK entry: {:?}", e);
            arch::halt();
        }
    };

    let file_offset = entry.file_offset as usize;
    let file_size = entry.file_size as usize;

    log_info!("BOOT", "=> Size    : {:#010x} bytes", file_size);

    if file_size == 0 {
        log_error!("BOOT", "Segment file_size is zero!");
        arch::halt();
    }

    log_info!("BOOT", "Extracting payload to entry point...");
    let payload_src = unsafe { ohc_base.add(file_offset) };
    let payload_dst = entry_point as *mut u8;

    for i in 0..file_size {
        unsafe {
            let val = core::ptr::read_volatile(payload_src.add(i));
            core::ptr::write_volatile(payload_dst.add(i), val);
        }
    }

    log_info!("BOOT", "Jumping to HNX Kernel...");

    let mut bootfs_size = 0usize;

    unsafe {
        let base = bootfs_pa as *const u8;
        if core::slice::from_raw_parts(base, 8) == b"HNXF_VFS" {
            let count_bytes = core::slice::from_raw_parts(base.add(8), 8);
            let mut count_arr = [0u8; 8];
            count_arr.copy_from_slice(count_bytes);
            let count = u64::from_le_bytes(count_arr) as usize;

            let mut max_end = 16;
            for i in 0..count {
                let entry_offset = 16 + i * 144;
                let path_end = entry_offset + 128;

                let mut off_bytes = [0u8; 8];
                let src_off = core::slice::from_raw_parts(base.add(path_end), 8);
                off_bytes.copy_from_slice(src_off);
                let file_offset = u64::from_le_bytes(off_bytes) as usize;

                let mut sz_bytes = [0u8; 8];
                let src_sz = core::slice::from_raw_parts(base.add(path_end + 8), 8);
                sz_bytes.copy_from_slice(src_sz);
                let file_size = u64::from_le_bytes(sz_bytes) as usize;

                let end = file_offset + file_size;
                if end > max_end {
                    max_end = end;
                }
            }
            bootfs_size = max_end;
        }
    }

    log_info!(
        "BOOT",
        "=> BootFS : PA=0x{:x}, Size={} bytes",
        bootfs_pa,
        bootfs_size
    );

    unsafe {
        let kernel_entry: extern "C" fn(dtb: *const u8, bootfs_pa: usize, bootfs_size: usize) -> ! =
            core::mem::transmute(payload_dst);
        kernel_entry(dtb_to_use, bootfs_pa, bootfs_size);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log_error!("BOOT", "PANIC: {}", info);
    arch::halt();
}
