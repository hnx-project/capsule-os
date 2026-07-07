#![no_std]
#![no_main]

mod arch;
mod platform;

use fdt::Fdt;

// ----------------------------------------------------------------------------
// 平台无关的通用 Bootloader 逻辑
// ----------------------------------------------------------------------------
#[no_mangle]
extern "C" fn rust_main(dtb_ptr: *const u8) -> ! {
    // 初始化平台硬件 (比如外设，暂为空)
    platform::init();

    log_info!("BOOT", "Booting v{}...", env!("CARGO_PKG_VERSION"));

    // 1. FDT 解析
    let dtb_to_use = if dtb_ptr.is_null() {
        // QEMU 未通过 x0 传入 DTB，尝试从已知的固定地址查找
        // QEMU virt 默认在 high memory 放置 DTB，但更可靠的方案是让 QEMU 通过 loader 加载
        let candidate = platform::DTB_FALLBACK_ADDR as *const u8;
        if let Ok(_fdt) = unsafe { Fdt::from_ptr(candidate) } {
            log_info!("BOOT", "DTB found at fallback addr {:#x}.", platform::DTB_FALLBACK_ADDR);
            candidate
        } else {
            log_warn!("BOOT", "DTB not found.");
            core::ptr::null()
        }
    } else {
        if let Ok(_fdt) = unsafe { Fdt::from_ptr(dtb_ptr) } {
            log_info!("BOOT", "DTB parsed successfully at {:#x}.", dtb_ptr as usize);
        } else {
            log_warn!("BOOT", "DTB magic invalid at {:#x}.", dtb_ptr as usize);
        }
        dtb_ptr
    };

    // 2. 解析 OHC Header
    let ohc_base = platform::OHC_BASE as *const u8;
    let mut header = [0u8; 32];
    for i in 0..32 {
        header[i] = unsafe { core::ptr::read_volatile(ohc_base.add(i)) };
    }

    // 检查 Magic 字段 ("OHLK")
    if &header[0..4] != b"OHLK" {
        log_error!(
            "BOOT",
            "Invalid OHC magic! Found: {}{}{}{}",
            header[0] as char,
            header[1] as char,
            header[2] as char,
            header[3] as char
        );
        log_error!("BOOT", "Halting CPU.");
        arch::halt();
    }

    // 提取字段 (使用小端序)
    let version = u16::from_le_bytes([header[4], header[5]]);
    let entry = u64::from_le_bytes([
        header[6], header[7], header[8], header[9],
        header[10], header[11], header[12], header[13],
    ]);
    let segment_count = u16::from_le_bytes([header[14], header[15]]);
    let size = u32::from_le_bytes([header[18], header[19], header[20], header[21]]);

    log_info!("BOOT", "Valid OHC Image Found!");
    log_info!("BOOT", "=> Version : {:#06x}", version);
    log_info!("BOOT", "=> Entry   : {:#018x}", entry);
    log_info!("BOOT", "=> Segments: {}", segment_count);
    log_info!("BOOT", "=> Size    : {:#010x} bytes", size);

    // 3. 将内核 Payload 拷贝 to 真实运行地址 (这里直接按 entry 地址来算)
    log_info!("BOOT", "Extracting payload to entry point...");
    let payload_src = unsafe { ohc_base.add(32 + (segment_count as usize) * 24) };
    let payload_dst = entry as *mut u8;

    for i in 0..(size as usize) {
        unsafe {
            let val = core::ptr::read_volatile(payload_src.add(i));
            core::ptr::write_volatile(payload_dst.add(i), val);
        }
    }

    // 4. 跳转到 HNX 内核！
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
