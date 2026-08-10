#![no_main]
#![no_std]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::media::file::File;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::security::MemoryProtection;
use uefi::table::boot::MemoryAttribute;
use log::{info, error, warn};

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use uefi::proto::media::file::{FileMode, FileAttribute, FileType};

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DriverHandoff {
    pub name: [u8; 32],
    pub paddr: u64,
    pub size: u64,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DriverTableHandoff {
    pub count: u64,
    pub drivers: [DriverHandoff; 16],
}

#[derive(Debug, Clone)]
pub struct DriverNode {
    pub name: String,
    pub path_name: String,
    pub dependencies: Vec<String>,
    pub paddr: u64,
    pub size: u64,
}

fn parse_metadata_field(toml: &str, key: &str) -> Option<String> {
    if let Some(idx) = toml.find(key) {
        let rest = &toml[idx + key.len()..];
        if let Some(start_quote) = rest.find('"') {
            let rest2 = &rest[start_quote + 1..];
            if let Some(end_quote) = rest2.find('"') {
                return Some(String::from(&rest2[..end_quote]));
            }
        }
    }
    None
}

fn parse_metadata_dependencies(toml: &str) -> Vec<String> {
    let mut deps = Vec::new();
    if let Some(idx) = toml.find("dependencies = [") {
        let rest = &toml[idx + "dependencies = [".len()..];
        if let Some(end_bracket) = rest.find(']') {
            let deps_str = &rest[..end_bracket];
            for item in deps_str.split(',') {
                let trimmed = item.trim().trim_matches('"').trim_matches('\'').trim();
                if !trimmed.is_empty() {
                    deps.push(String::from(trimmed));
                }
            }
        }
    }
    deps
}

fn topological_sort(nodes: &mut Vec<DriverNode>) -> core::result::Result<Vec<DriverNode>, &'static str> {
    let mut sorted = Vec::new();
    let mut visited = BTreeMap::new(); // name -> State (0 = visiting, 1 = visited)
    
    fn dfs(
        node_idx: usize,
        nodes: &Vec<DriverNode>,
        visited: &mut BTreeMap<String, u8>,
        sorted: &mut Vec<DriverNode>,
    ) -> core::result::Result<(), &'static str> {
        let node = &nodes[node_idx];
        if let Some(&state) = visited.get(&node.name) {
            if state == 0 {
                return Err("Circular dependency detected!");
            }
            return Ok(());
        }
        
        visited.insert(node.name.clone(), 0); // visiting
        
        for dep in &node.dependencies {
            if let Some(dep_idx) = nodes.iter().position(|n| &n.name == dep) {
                dfs(dep_idx, nodes, visited, sorted)?;
            }
        }
        
        visited.insert(node.name.clone(), 1); // visited
        sorted.push(nodes[node_idx].clone());
        Ok(())
    }
    
    for i in 0..nodes.len() {
        if !visited.contains_key(&nodes[i].name) {
            dfs(i, nodes, &mut visited, &mut sorted)?;
        }
    }
    
    Ok(sorted)
}

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
    let mut handoff_table_pa: u64 = 0;
    let mut handoff_table_size: u64 = 0;
    let mut bootfs_pa: usize = 0;
    let mut bootfs_size: usize = 0;

    // Inner lexical block to contain all file loading and parsing borrows
    {
        let boot_services = system_table.boot_services();
        
        // Try to locate RootFS partition by searching all SimpleFileSystem handles first
        let mut root = None;
        if let Ok(handles) = boot_services.locate_handle_buffer(uefi::table::boot::SearchType::AllHandles) {
            for &handle in &*handles {
                if let Ok(mut fs) = boot_services.open_protocol_exclusive::<SimpleFileSystem>(handle) {
                    if let Ok(mut vol) = fs.open_volume() {
                        if vol.open(cstr16!("boot\\KERNEL"), FileMode::Read, FileAttribute::empty()).is_ok() {
                            info!("  Located CapsuleOS RootFS partition via SimpleFileSystem!");
                            root = Some(vol);
                            break;
                        }
                    }
                }
            }
        }

        // Fallback: If not found via search, use the original boot device SimpleFileSystem
        let mut root = match root {
            Some(r) => r,
            None => {
                warn!("  Could not locate partition containing boot\\KERNEL via SimpleFileSystem protocol search. Falling back to boot device SimpleFileSystem.");
                let loaded_image = match boot_services.open_protocol_exclusive::<uefi::proto::loaded_image::LoadedImage>(image_handle) {
                    Ok(li) => li,
                    Err(_) => {
                        error!("Failed to open LoadedImage protocol!");
                        return Status::LOAD_ERROR;
                    }
                };
                let device = loaded_image.device();
                let mut fs = match boot_services.open_protocol_exclusive::<SimpleFileSystem>(device) {
                    Ok(fs) => fs,
                    Err(_) => {
                        error!("SimpleFileSystem protocol not found on boot device!");
                        return Status::LOAD_ERROR;
                    }
                };
                match fs.open_volume() {
                    Ok(v) => v,
                    Err(_) => {
                        error!("Failed to open Root Volume of Boot Partition!");
                        return Status::LOAD_ERROR;
                    }
                }
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
        let mut info_buf = [0u8; 256];
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
                let mut dtb_info_buf = [0u8; 256];
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

        let mut driver_nodes = Vec::new();

        // 8a. Dynamic directory scanning for .pill driver bundles
        if let Ok(dir_handle) = root.open(
            cstr16!("extensions"),
            uefi::proto::media::file::FileMode::Read,
            uefi::proto::media::file::FileAttribute::DIRECTORY
        ) {
            if let Ok(uefi::proto::media::file::FileType::Dir(mut dir)) = dir_handle.into_type() {
                while let Ok(Some(entry)) = dir.read_entry_boxed() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string();
                    if name_str == "." || name_str == ".." {
                        continue;
                    }
                    if name_str.ends_with(".pillsmod") {
                        info!("  Discovered .pillsmod driver bundle: {}", name_str);
                        
                        // Parse metadata.toml inside this directory
                        let mut meta_content = String::new();
                        let meta_path_str = alloc::format!("extensions\\{}\\metadata.toml", name_str);
                        let meta_path = uefi::CString16::try_from(meta_path_str.as_str()).unwrap();
                        
                        let meta_open_res = root.open(&meta_path, FileMode::Read, FileAttribute::empty());
                        match meta_open_res {
                            Ok(meta_handle) => {
                                info!("    Successfully opened metadata.toml!");
                                match meta_handle.into_type() {
                                    Ok(FileType::Regular(mut meta_file)) => {
                                        // Read up to 1024 bytes directly without calling get_info
                                        let mut meta_bytes = [0u8; 1024];
                                        if let Ok(read_bytes) = meta_file.read(&mut meta_bytes) {
                                            if read_bytes > 0 {
                                                if let Ok(meta_str) = core::str::from_utf8(&meta_bytes[..read_bytes]) {
                                                    meta_content = String::from(meta_str);
                                                    info!("      Successfully read metadata.toml ({} bytes)!", read_bytes);
                                                }
                                            }
                                        }
                                    }
                                    Ok(FileType::Dir(_)) => {
                                        error!("      metadata.toml is a Directory!");
                                    }
                                    Err(e) => {
                                        error!("      into_type failed: {:?}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("    Failed to open metadata.toml at {}: {:?}", meta_path_str, e);
                            }
                        }

                        if meta_content.is_empty() {
                            error!("    Failed to load metadata.toml for {}", name_str);
                            continue;
                        }

                        let driver_name = parse_metadata_field(&meta_content, "name = ").unwrap_or_else(|| String::from(&name_str[..name_str.len() - 5]));
                        let dependencies = parse_metadata_dependencies(&meta_content);

                        // Load driver inside this directory
                        let raw_path_str = alloc::format!("extensions\\{}\\driver", name_str);
                        let raw_path = uefi::CString16::try_from(raw_path_str.as_str()).unwrap();
                        
                        let mut paddr: u64 = 0;
                        let mut size: u64 = 0;

                        let raw_open_res = root.open(&raw_path, FileMode::Read, FileAttribute::empty());
                        match raw_open_res {
                            Ok(raw_handle) => {
                                match raw_handle.into_type() {
                                    Ok(FileType::Regular(mut raw_file)) => {
                                        let mut raw_info_buf_aligned = [0u64; 32];
                                        let raw_info_buf = unsafe {
                                            core::slice::from_raw_parts_mut(raw_info_buf_aligned.as_mut_ptr() as *mut u8, 256)
                                        };
                                        let info_res = raw_file.get_info::<uefi::proto::media::file::FileInfo>(raw_info_buf);
                                        match info_res {
                                            Ok(raw_info) => {
                                                let r_size = raw_info.file_size() as usize;
                                                let r_pages = (r_size + 4095) / 4096;
                                                if let Ok(r_addr) = boot_services.allocate_pages(
                                                    uefi::table::boot::AllocateType::AnyPages,
                                                    uefi::table::boot::MemoryType::LOADER_DATA,
                                                    r_pages
                                                ) {
                                                    let raw_buf = unsafe { core::slice::from_raw_parts_mut(r_addr as *mut u8, r_size) };
                                                    if raw_file.read(raw_buf).is_ok() {
                                                        paddr = r_addr;
                                                        size = r_size as u64;
                                                        info!("    [SUCCESS] Loaded driver payload ({:#x}, {} bytes)", r_addr, r_size);
                                                    } else {
                                                        error!("      Failed to read driver bytes!");
                                                    }
                                                } else {
                                                    error!("      Failed to allocate pages for driver!");
                                                }
                                            }
                                            Err(e) => {
                                                error!("      get_info failed on driver: {:?}", e);
                                            }
                                        }
                                    }
                                    _ => {
                                        error!("      driver is not a Regular file!");
                                    }
                                }
                            }
                            Err(e) => {
                                error!("    Failed to open driver at {}: {:?}", raw_path_str, e);
                            }
                        }

                        if paddr != 0 && size > 0 {
                            driver_nodes.push(DriverNode {
                                name: driver_name,
                                path_name: name_str,
                                dependencies,
                                paddr,
                                size,
                            });
                        }
                    }
                }
            }
        }

        // 8a2. Topological dependency resolution
        let sorted_drivers = match topological_sort(&mut driver_nodes) {
            Ok(s) => s,
            Err(e) => {
                error!("  [FATAL] Driver dependency sort failed: {}", e);
                return Status::LOAD_ERROR;
            }
        };

        // 8a3. Build dynamic DriverTableHandoff in boot loader memory
        if !sorted_drivers.is_empty() {
            let table_pages = (core::mem::size_of::<DriverTableHandoff>() + 4095) / 4096;
            if let Ok(table_addr) = boot_services.allocate_pages(
                uefi::table::boot::AllocateType::AnyPages,
                uefi::table::boot::MemoryType::LOADER_DATA,
                table_pages
            ) {
                let handoff_table = unsafe { &mut *(table_addr as *mut DriverTableHandoff) };
                handoff_table.count = sorted_drivers.len() as u64;
                
                let drv_count = handoff_table.count;
                info!("  Constructing Driver Handoff Table ({} drivers):", drv_count);
                for (i, drv) in sorted_drivers.iter().enumerate() {
                    let mut name_bytes = [0u8; 32];
                    let src_bytes = drv.name.as_bytes();
                    let len = src_bytes.len().min(31);
                    name_bytes[..len].copy_from_slice(&src_bytes[..len]);

                    handoff_table.drivers[i] = DriverHandoff {
                        name: name_bytes,
                        paddr: drv.paddr,
                        size: drv.size,
                    };
                    info!("    [{}] name: {}, paddr: {:#x}, size: {}", i, drv.name, drv.paddr, drv.size);
                }
                
                handoff_table_pa = table_addr;
                handoff_table_size = core::mem::size_of::<DriverTableHandoff>() as u64;
            }
        }

        // 8a4. Dynamically pack 9 bootstrap services from RootFS into an HNXF_VFS BootFS RAM package
        {
            use uefi::proto::media::file::FileInfo;

            let bootstrap_files = [
                ("system\\bin\\loader", "system/bin/loader"),
                ("system\\bin\\servicesd", "system/bin/servicesd"),
                ("system\\bin\\fileagent", "system/bin/fileagent"),
                ("system\\bin\\procmgr", "system/bin/procmgr"),
                ("system\\bin\\devmgr", "system/bin/devmgr"),
                ("system\\bin\\netd", "system/bin/netd"),
                ("system\\bin\\gpud", "system/bin/gpud"),
                ("system\\bin\\inputd", "system/bin/inputd"),
                ("system\\bin\\touchd", "system/bin/touchd"),
            ];

            let mut vfs_entries = alloc::vec::Vec::new();
            let mut total_vfs_size = 16 + bootstrap_files.len() * 144;

            info!("  Scanning for {} bootstrap services...", bootstrap_files.len());
            for &(phys_path, virt_path) in &bootstrap_files {
                let path_c16 = uefi::CString16::try_from(phys_path).unwrap();
                if let Ok(file_handle) = root.open(&path_c16, FileMode::Read, FileAttribute::empty()) {
                    if let Ok(FileType::Regular(mut file)) = file_handle.into_type() {
                        let mut info_buf = [0u8; 256];
                        if let Ok(file_info) = file.get_info::<FileInfo>(&mut info_buf) {
                            let size = file_info.file_size() as usize;
                            let aligned_size = (size + 15) & !15; // 16-byte aligned size
                            vfs_entries.push((phys_path, virt_path, size, file));
                            total_vfs_size += aligned_size;
                        }
                    }
                } else {
                    warn!("    Required bootstrap service '{}' not found on RootFS!", phys_path);
                }
            }

            if vfs_entries.is_empty() {
                error!("No bootstrap services found! Cannot build BootFS.");
                return Status::LOAD_ERROR;
            }

            info!("    Total BootFS dynamic RAM package size: {} bytes. Allocating pages...", total_vfs_size);
            let bootfs_pages = (total_vfs_size + 4095) / 4096;
            if let Ok(bootfs_addr) = boot_services.allocate_pages(
                uefi::table::boot::AllocateType::AnyPages,
                uefi::table::boot::MemoryType::LOADER_DATA,
                bootfs_pages
            ) {
                let bootfs_buf = unsafe { core::slice::from_raw_parts_mut(bootfs_addr as *mut u8, total_vfs_size) };
                bootfs_buf.fill(0);

                // 1. Superblock
                bootfs_buf[0..8].copy_from_slice(b"HNXF_VFS");
                let entry_count = vfs_entries.len() as u64;
                bootfs_buf[8..16].copy_from_slice(&entry_count.to_le_bytes());

                // 2. Pack files and build headers
                let mut current_offset = 16 + vfs_entries.len() * 144;
                for (idx, (_phys_path, virt_path, size_ref, file)) in vfs_entries.iter_mut().enumerate() {
                    let size = *size_ref;
                    let entry_offset = 16 + idx * 144;

                    // Path (128 bytes)
                    let virt_bytes = virt_path.as_bytes();
                    let copy_len = virt_bytes.len().min(127);
                    bootfs_buf[entry_offset..entry_offset + copy_len].copy_from_slice(&virt_bytes[..copy_len]);

                    // Offset (8 bytes)
                    let offset_le = (current_offset as u64).to_le_bytes();
                    bootfs_buf[entry_offset + 128..entry_offset + 136].copy_from_slice(&offset_le);

                    // Size (8 bytes)
                    let size_le = (size as u64).to_le_bytes();
                    bootfs_buf[entry_offset + 136..entry_offset + 144].copy_from_slice(&size_le);

                    // Read file data directly into dynamic BootFS memory buffer!
                    let data_slice = &mut bootfs_buf[current_offset..current_offset + size];
                    if file.read(data_slice).is_err() {
                        error!("      Failed to read data for service: {}", virt_path);
                        return Status::LOAD_ERROR;
                    }

                    info!("    Packed service '{}' [offset: {}, size: {}]", virt_path, current_offset, size);

                    let aligned_size = (size + 15) & !15;
                    current_offset += aligned_size;
                }

                bootfs_pa = bootfs_addr as usize;
                bootfs_size = total_vfs_size;
                info!("  [SUCCESS] Dynamic BootFS packed successfully at physical {:#x} ({} bytes)", bootfs_pa, bootfs_size);
            } else {
                error!("Failed to allocate LOADER_DATA pages for dynamic BootFS!");
                return Status::OUT_OF_RESOURCES;
            }
        }
        
        // 8b. Dynamically clear EXECUTE_PROTECT (NX) using standard MemoryProtection protocol
        if let Ok(handle) = boot_services.get_handle_for_protocol::<MemoryProtection>() {
            if let Ok(mp) = boot_services.open_protocol_exclusive::<MemoryProtection>(handle) {
                let mp_ref = mp.get_mut().unwrap();
                info!("  MemoryProtection protocol located. Clearing EXECUTE_PROTECT on kernel region...");
                let aligned_size = (payload_size as u64 + 4095) & !4095;
                let range = entry_point .. entry_point + aligned_size;
                if mp_ref.clear_memory_attributes(range, MemoryAttribute::EXECUTE_PROTECT).is_ok() {
                    info!("  [SUCCESS] Execute-Never (NX) attribute cleared successfully on kernel memory!");
                } else {
                    error!("  Failed to clear EXECUTE_PROTECT attribute on the kernel region!");
                }

                if handoff_table_pa != 0 {
                    let handoff_table = unsafe { &*(handoff_table_pa as *const DriverTableHandoff) };
                    for i in 0..handoff_table.count as usize {
                        let drv = &handoff_table.drivers[i];
                        info!("  Clearing EXECUTE_PROTECT on driver '{}'...", core::str::from_utf8(&drv.name).unwrap_or("unknown").trim_matches('\0'));
                        let pill_aligned_size = (drv.size + 4095) & !4095;
                        let pill_range = drv.paddr .. drv.paddr + pill_aligned_size;
                        if mp_ref.clear_memory_attributes(pill_range, MemoryAttribute::EXECUTE_PROTECT).is_ok() {
                            info!("    [SUCCESS] Execute-Never (NX) attribute cleared successfully!");
                        } else {
                            error!("    Failed to clear EXECUTE_PROTECT attribute!");
                        }
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

    if handoff_table_pa != 0 {
        let handoff_table = unsafe { &*(handoff_table_pa as *const DriverTableHandoff) };
        for i in 0..handoff_table.count as usize {
            let drv = &handoff_table.drivers[i];
            unsafe {
                let mut addr = drv.paddr;
                let end = drv.paddr + drv.size;
                while addr < end {
                    core::arch::asm!("dc cvac, {0}", in(reg) addr);
                    addr += 64;
                }
                core::arch::asm!("dsb ish");
                
                let mut addr = drv.paddr;
                while addr < end {
                    core::arch::asm!("ic ivau, {0}", in(reg) addr);
                    addr += 64;
                }
                core::arch::asm!("dsb ish", "isb");
            }
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
            in("x1") bootfs_pa,  // Pass bootfs_pa in x1
            in("x2") bootfs_size,  // Pass bootfs_size in x2
            in("x3") handoff_table_pa as usize,   // Pass handoff_table_pa in x3
            in("x4") handoff_table_size as usize, // Pass handoff_table_size in x4
            options(noreturn)
        );
    }
}
