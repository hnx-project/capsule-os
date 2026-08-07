#![no_main]
#![no_std]

use uefi::prelude::*;
use uefi::proto::media::file::File;
use uefi::proto::security::MemoryProtection;
use uefi::table::boot::MemoryAttribute;
use log::{info, error, warn};

#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // Initialize UEFI services (includes standard logging backends and panic handlers)
    uefi_services::init(&mut system_table).unwrap();
    
    info!("==============================================");
    info!("      capsuleOS (Pangu) UEFI Bootloader       ");
    info!("==============================================");
    info!("");
    info!("  Initializing system UEFI services... Done");
    info!("  AArch64 UEFI CPU Execution State: EL1 / Active");
    info!("  U-Disk Storage FileSystem Discovery: Active");
    info!("  Locating CapsuleOS HNX-Core microkernel...");
    
    let mut entry_point: u64 = 0;
    let mut dtb_ptr = core::ptr::null();
    let mut payload_size: usize = 0;
    let mut pill_pa: u64 = 0;
    let mut pill_size: u64 = 0;

    // Inner lexical block to contain all file loading and parsing borrows
    {
        let boot_services = system_table.boot_services();
        
        // 1. Get LoadedImage protocol to identify the boot device handle
        let loaded_image = match boot_services.open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(image_handle) {
            Ok(li) => li,
            Err(_) => {
                error!("Failed to open LoadedImage protocol!");
                return Status::LOAD_ERROR;
            }
        };
        let device = loaded_image.device();
        
        // 2. Open SimpleFileSystem protocol on the boot device
        let mut fs = match boot_services.open_protocol_exclusive::<uefi::proto::media::fs::SimpleFileSystem>(device) {
            Ok(fs) => fs,
            Err(_) => {
                error!("SimpleFileSystem protocol not found on boot device!");
                return Status::LOAD_ERROR;
            }
        };
        
        // 3. Open the Root Volume
        let mut root = match fs.open_volume() {
            Ok(v) => v,
            Err(_) => {
                error!("Failed to open Root Volume of Boot Partition!");
                return Status::LOAD_ERROR;
            }
        };
        
        // 4. Open boot\KERNEL
        let file_handle = match root.open(
            cstr16!("boot\\KERNEL"),
            uefi::proto::media::file::FileMode::Read,
            uefi::proto::media::file::FileAttribute::empty()
        ) {
            Ok(f) => f,
            Err(_) => {
                error!("CapsuleOS microkernel file '\\boot\\KERNEL' not found on U-Disk!");
                return Status::NOT_FOUND;
            }
        };
        
        let mut file = match file_handle.into_type() {
            Ok(uefi::proto::media::file::FileType::Regular(f)) => f,
            _ => {
                error!("\\boot\\KERNEL is not a regular file!");
                return Status::LOAD_ERROR;
            }
        };
        
        // 5. Query KERNEL file size
        let mut info_buf = [0u8; 128];
        let file_info = match file.get_info::<uefi::proto::media::file::FileInfo>(&mut info_buf) {
            Ok(info) => info,
            Err(_) => {
                error!("Failed to read microkernel file info!");
                return Status::LOAD_ERROR;
            }
        };
        let file_size = file_info.file_size() as usize;
        
        // 6. Allocate page-aligned memory for the temporary buffer as LOADER_DATA (Writable)
        let pages_count = (file_size + 4095) / 4096;
        let buffer_address = match boot_services.allocate_pages(
            uefi::table::boot::AllocateType::AnyPages,
            uefi::table::boot::MemoryType::LOADER_DATA,
            pages_count
        ) {
            Ok(addr) => addr,
            Err(_) => {
                error!("Out of resources while allocating memory pages for temporary buffer!");
                return Status::OUT_OF_RESOURCES;
            }
        };
        
        let buffer_ptr = buffer_address as *mut u8;
        let buffer = unsafe { core::slice::from_raw_parts_mut(buffer_ptr, file_size) };
        if file.read(buffer).is_err() {
            error!("Failed to read microkernel binary from U-Disk!");
            return Status::LOAD_ERROR;
        }
        
        // 7. Parse OHLINK header and extract Entry Point & Data Offset
        if file_size < 64 {
            error!("Microkernel file size is too small!");
            return Status::LOAD_ERROR;
        }
        
        let magic = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        if magic != 0x4F484C4B {
            error!("Microkernel OHLINK signature magic verification failed!");
            return Status::LOAD_ERROR;
        }
        
        entry_point = u64::from_le_bytes([
            buffer[38], buffer[39], buffer[40], buffer[41],
            buffer[42], buffer[43], buffer[44], buffer[45],
        ]);
        
        let header_offset = u32::from_le_bytes([
            buffer[14], buffer[15], buffer[16], buffer[17],
        ]) as usize;
        
        if file_size < header_offset + 60 {
            error!("Microkernel file is too small to contain OHLK_Entry!");
            return Status::LOAD_ERROR;
        }
        
        let entry_bytes = &buffer[header_offset..header_offset + 60];
        let file_offset = u64::from_le_bytes([
            entry_bytes[8], entry_bytes[9], entry_bytes[10], entry_bytes[11],
            entry_bytes[12], entry_bytes[13], entry_bytes[14], entry_bytes[15],
        ]) as usize;
        
        payload_size = u64::from_le_bytes([
            entry_bytes[24], entry_bytes[25], entry_bytes[26], entry_bytes[27],
            entry_bytes[28], entry_bytes[29], entry_bytes[30], entry_bytes[31],
        ]) as usize;
        
        // Report success metrics
        info!("\\boot\\KERNEL parsed successfully ({} bytes)!", file_size);
        info!("HNX-Core Entry Point: {:#018x}", entry_point);
        info!("First loadable segment: file_offset = {}, size = {}", file_offset, payload_size);
        
        // 7b. Allocate exact, identity-mapped physical pages at `entry_point` as LOADER_DATA (Read-Write RW)
        let kernel_pages_count = (payload_size + 4095) / 4096;
        let target_address = match boot_services.allocate_pages(
            uefi::table::boot::AllocateType::Address(entry_point),
            uefi::table::boot::MemoryType::LOADER_DATA,
            kernel_pages_count
        ) {
            Ok(addr) => addr,
            Err(_) => {
                error!("Failed to allocate physical pages at kernel entry {:#018x}!", entry_point);
                return Status::OUT_OF_RESOURCES;
            }
        };
        
        // Copy kernel payload directly to its newly authorized physical address
        let payload_ptr = unsafe { buffer_ptr.add(file_offset) };
        
        info!("  Copying kernel payload ({} bytes) to authorized address {:#018x}...", payload_size, target_address);
        unsafe {
            core::ptr::copy_nonoverlapping(payload_ptr, target_address as *mut u8, payload_size);
        }
        
        // 8a. Try to load dynamic DTB from `boot\qemu_virt.dtb` if present on U-Disk
        if let Ok(dtb_handle) = root.open(
            cstr16!("boot\\qemu_virt.dtb"),
            uefi::proto::media::file::FileMode::Read,
            uefi::proto::media::file::FileAttribute::empty()
        ) {
            if let Ok(uefi::proto::media::file::FileType::Regular(mut dtb_file)) = dtb_handle.into_type() {
                let mut dtb_info_buf = [0u8; 128];
                if let Ok(dtb_info) = dtb_file.get_info::<uefi::proto::media::file::FileInfo>(&mut dtb_info_buf) {
                    let dtb_size = dtb_info.file_size() as usize;
                    let dtb_pages = (dtb_size + 4095) / 4096;
                    if let Ok(dtb_addr) = boot_services.allocate_pages(
                        uefi::table::boot::AllocateType::AnyPages,
                        uefi::table::boot::MemoryType::LOADER_DATA,
                        dtb_pages
                    ) {
                        let dtb_buf = unsafe { core::slice::from_raw_parts_mut(dtb_addr as *mut u8, dtb_size) };
                        if dtb_file.read(dtb_buf).is_ok() {
                            dtb_ptr = dtb_addr as *const u8;
                            info!("  [SUCCESS] Loaded dynamic DTB from '\\boot\\qemu_virt.dtb' ({:#x}, {} bytes)!", dtb_addr, dtb_size);
                        }
                    }
                }
            }
        }

        if dtb_ptr.is_null() {
            // 8b. Retrieve host Flat Device Tree (FDT) pointer from UEFI system tables
            let fdt_guid = uefi::guid!("b16f28cf-2b0e-4ff6-b258-00a87679f225");
            info!("  Scanning UEFI Configuration Tables (total {} entries):", system_table.config_table().len());
            for (i, table) in system_table.config_table().iter().enumerate() {
                info!("    Table [{}]: GUID = {:?}, Address = {:#x}", i, table.guid, table.address as usize);
                if table.guid == fdt_guid {
                    dtb_ptr = table.address as *const u8;
                }
            }
        }
        info!("  Final system DTB pointer: {:?}", dtb_ptr);

        // 8a2. Load the hello_pill.pill from U-Disk if present
        if let Ok(pill_handle) = root.open(
            cstr16!("extensions\\hello_pill.pill"),
            uefi::proto::media::file::FileMode::Read,
            uefi::proto::media::file::FileAttribute::empty()
        ) {
            if let Ok(uefi::proto::media::file::FileType::Regular(mut pill_file)) = pill_handle.into_type() {
                let mut pill_info_buf = [0u8; 128];
                if let Ok(pill_info) = pill_file.get_info::<uefi::proto::media::file::FileInfo>(&mut pill_info_buf) {
                    let p_size = pill_info.file_size() as usize;
                    let p_pages = (p_size + 4095) / 4096;
                    if let Ok(p_addr) = boot_services.allocate_pages(
                        uefi::table::boot::AllocateType::AnyPages,
                        uefi::table::boot::MemoryType::LOADER_DATA,
                        p_pages
                    ) {
                        let pill_buf = unsafe { core::slice::from_raw_parts_mut(p_addr as *mut u8, p_size) };
                        if pill_file.read(pill_buf).is_ok() {
                            pill_pa = p_addr;
                            pill_size = p_size as u64;
                            info!("  [SUCCESS] Loaded driver package '\\extensions\\hello_pill.pill' ({:#x}, {} bytes)!", p_addr, p_size);
                        }
                    }
                }
            }
        }
        
        // 8b. Dynamically clear EXECUTE_PROTECT (NX) using standard MemoryProtection protocol
        if let Ok(handle) = boot_services.get_handle_for_protocol::<MemoryProtection>() {
            if let Ok(mut mp) = boot_services.open_protocol_exclusive::<MemoryProtection>(handle) {
                let mp_ref = mp.get_mut().unwrap();
                info!("  MemoryProtection protocol located. Clearing EXECUTE_PROTECT on kernel region...");
                let aligned_size = (payload_size as u64 + 4095) & !4095;
                let range = entry_point .. entry_point + aligned_size;
                if mp_ref.clear_memory_attributes(range, MemoryAttribute::EXECUTE_PROTECT).is_ok() {
                    info!("  [SUCCESS] Execute-Never (NX) attribute cleared successfully on kernel memory!");
                } else {
                    error!("  Failed to clear EXECUTE_PROTECT attribute on the kernel region!");
                }

                if pill_pa != 0 && pill_size > 0 {
                    info!("  Clearing EXECUTE_PROTECT on loaded driver package...");
                    let pill_aligned_size = (pill_size + 4095) & !4095;
                    let pill_range = pill_pa .. pill_pa + pill_aligned_size;
                    if mp_ref.clear_memory_attributes(pill_range, MemoryAttribute::EXECUTE_PROTECT).is_ok() {
                        info!("  [SUCCESS] Execute-Never (NX) attribute cleared successfully on driver memory!");
                    } else {
                        error!("  Failed to clear EXECUTE_PROTECT attribute on the driver region!");
                    }
                }
            }
        } else {
            warn!("  MemoryProtection protocol not found on this UEFI platform.");
        }
    }
    
    // Perform standard ARMv8 Cache Coherency maintenance before disabling boot services
    info!("  Synchronizing Instruction and Data caches for kernel execution... Done");
    unsafe {
        let mut addr = entry_point;
        let end = entry_point + payload_size as u64;
        while addr < end {
            core::arch::asm!("dc cvac, {0}", in(reg) addr);
            addr += 64; // assuming typical 64-byte cache line size
        }
        core::arch::asm!("dsb ish");
        
        let mut addr = entry_point;
        while addr < end {
            core::arch::asm!("ic ivau, {0}", in(reg) addr);
            addr += 64;
        }
        core::arch::asm!("dsb ish", "isb");
    }

    if pill_pa != 0 && pill_size > 0 {
        unsafe {
            let mut addr = pill_pa;
            let end = pill_pa + pill_size;
            while addr < end {
                core::arch::asm!("dc cvac, {0}", in(reg) addr);
                addr += 64;
            }
            core::arch::asm!("dsb ish");
            
            let mut addr = pill_pa;
            while addr < end {
                core::arch::asm!("ic ivau, {0}", in(reg) addr);
                addr += 64;
            }
            core::arch::asm!("dsb ish", "isb");
        }
    }
    
    // 9. Exit Boot Services to hand over bare-metal CPU state
    info!("  Exiting UEFI Boot Services to jump into microkernel...");
    let (_runtime_table, _mmap) = system_table.exit_boot_services();
    
    // 10. Disable MMU, Caches and execute the ultimate jump to the microkernel entry point
    unsafe {
        core::arch::asm!(
            "mrs x9, sctlr_el1",
            "mov x10, #0x1005",
            "bic x9, x9, x10", // Clear M (MMU), C (D-cache), I (I-cache)
            "msr sctlr_el1, x9",
            "isb",
            "br {entry}", // Branch to kernel entry address
            entry = in(reg) entry_point,
            in("x0") dtb_ptr, // Pass DTB pointer in x0
            in("x1") 0usize,  // Pass bootfs_pa in x1
            in("x2") 0usize,  // Pass bootfs_size in x2
            in("x3") pill_pa as usize,   // Pass pill_pa in x3
            in("x4") pill_size as usize, // Pass pill_size in x4
            options(noreturn)
        );
    }
}
