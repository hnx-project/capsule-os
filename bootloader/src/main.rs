#![no_std]
#![no_main]

mod arch;
mod platform;

use fdt::Fdt;

#[no_mangle]
extern "C" fn rust_main(dtb_ptr: *const u8) -> ! {
    platform::init();

    log_info!("BOOT", "Booting v{}...", env!("CARGO_PKG_VERSION"));

    let dtb_to_use = if dtb_ptr.is_null() {
        let candidate = platform::DTB_FALLBACK_ADDR as *const u8;
        if let Ok(_fdt) = unsafe { Fdt::from_ptr(candidate) } {
            log_info!(
                "BOOT",
                "DTB found at fallback addr {:#x}.",
                platform::DTB_FALLBACK_ADDR
            );
            candidate
        } else {
            log_warn!("BOOT", "DTB not found.");
            core::ptr::null()
        }
    } else {
        if let Ok(_fdt) = unsafe { Fdt::from_ptr(dtb_ptr) } {
            log_info!(
                "BOOT",
                "DTB parsed successfully at {:#x}.",
                dtb_ptr as usize
            );
        } else {
            log_warn!("BOOT", "DTB magic invalid at {:#x}.", dtb_ptr as usize);
        }
        dtb_ptr
    };

    let ohc_base = platform::OHC_BASE as *const u8;

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

    let bootfs_pa = platform::BOOTFS_BASE;
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
