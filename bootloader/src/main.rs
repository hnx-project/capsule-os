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

    let file_size = header.file_size as usize;
    let entry_point = header.entry_point;
    let data_offset = header.data_offset as usize;

    log_info!("BOOT", "Valid OHC Image Found!");
    log_info!(
        "BOOT",
        "=> Version : {}.{}",
        header.version_major,
        header.version_minor
    );
    log_info!("BOOT", "=> Entry   : {:#018x}", entry_point);
    log_info!("BOOT", "=> Segments: {}", header.header_count);
    log_info!("BOOT", "=> Size    : {:#010x} bytes", file_size);

    log_info!("BOOT", "Extracting payload to entry point...");
    let payload_src = unsafe { ohc_base.add(data_offset) };
    let payload_dst = entry_point as *mut u8;
    let payload_size = file_size - data_offset;

    for i in 0..payload_size {
        unsafe {
            let val = core::ptr::read_volatile(payload_src.add(i));
            core::ptr::write_volatile(payload_dst.add(i), val);
        }
    }

    log_info!("BOOT", "Jumping to HNX Kernel...");
    unsafe {
        let kernel_entry: extern "C" fn(dtb: *const u8) -> ! = core::mem::transmute(payload_dst);
        kernel_entry(dtb_to_use);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log_error!("BOOT", "PANIC: {}", info);
    arch::halt();
}
