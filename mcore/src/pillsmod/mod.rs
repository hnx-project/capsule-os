//! # 🛸 PillsMod (Kext) Module Loader & Registration Manager
//!
//! Handles kernel-space parsing, verification, and registration of dynamic
//! `.pill` driver bundles in the HNX Hybrid Kernel (EL1).
//! 
//! For Pangu 1.0-beta / S10, this serves as the foundational Pill Loader Stub,
//! verifying the "PILL" magic header, validating metadata offsets, and preparing
//! the payload for execution.

use shared::status::{Result, Status};

/// One driver handoff entry in the table.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DriverHandoff {
    pub name: [u8; 32],
    pub paddr: u64,
    pub size: u64,
}

/// Dynamic driver table handed off from UEFI bootloader.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DriverTableHandoff {
    pub count: u64,
    pub drivers: [DriverHandoff; 16],
}

/// The binary header structure at the very beginning of every `.pill` driver package.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct PillHeader {
    pub magic: [u8; 4],         // Must be b"PILL"
    pub version_major: u16,     // e.g. 1
    pub version_minor: u16,     // e.g. 0
    pub metadata_offset: u64,   // Offset to the metadata.toml/json text in the file
    pub metadata_size: u64,     // Size of the metadata text
    pub payload_offset: u64,    // Offset to the raw binary payload in the file
    pub payload_size: u64,      // Size of the raw binary payload
}

/// A parsed view of a `.pill` driver bundle in-kernel.
pub struct ParsedPill<'a> {
    pub header: PillHeader,
    pub metadata: &'a str,
    pub payload: &'a [u8],
}

impl<'a> ParsedPill<'a> {
    /// Parse and verify a dynamic `.pill` package from a raw memory buffer.
    pub fn parse(buf: &'a [u8]) -> Result<Self> {
        if buf.len() < core::mem::size_of::<PillHeader>() {
            crate::log_error!("PILLSMOD", "Buffer size ({}) is smaller than PillHeader", buf.len());
            return Err(Status::InvalidArgs);
        }

        // 1. Safe pointer transmutation of packed PillHeader
        let header_ptr = buf.as_ptr() as *const PillHeader;
        let header = unsafe { core::ptr::read_unaligned(header_ptr) };

        // 2. Verify magic number "PILL"
        if &header.magic != b"PILL" {
            crate::log_error!(
                "PILLSMOD",
                "Magic verification failed: expected 'PILL', got [{:02x}, {:02x}, {:02x}, {:02x}]",
                header.magic[0], header.magic[1], header.magic[2], header.magic[3]
            );
            return Err(Status::InvalidArgs);
        }

        let v_major = header.version_major;
        let v_minor = header.version_minor;
        crate::log_info!("PILLSMOD", "Discovered .pill driver pack (v{}.{})", v_major, v_minor);

        // 3. Extract and validate metadata slice
        let meta_start = header.metadata_offset as usize;
        let meta_end = meta_start + header.metadata_size as usize;
        if meta_end > buf.len() {
            crate::log_error!("PILLSMOD", "Metadata bounds [{}..{}] exceed buffer size ({})", meta_start, meta_end, buf.len());
            return Err(Status::InvalidArgs);
        }
        let metadata_raw = &buf[meta_start..meta_end];
        let metadata = core::str::from_utf8(metadata_raw)
            .map_err(|_| {
                crate::log_error!("PILLSMOD", "Failed to decode metadata as valid UTF-8");
                Status::InvalidArgs
            })?;

        // 4. Extract and validate executable payload slice
        let payload_start = header.payload_offset as usize;
        let payload_end = payload_start + header.payload_size as usize;
        if payload_end > buf.len() {
            crate::log_error!("PILLSMOD", "Payload bounds [{}..{}] exceed buffer size ({})", payload_start, payload_end, buf.len());
            return Err(Status::InvalidArgs);
        }
        let payload = &buf[payload_start..payload_end];

        let m_size = header.metadata_size;
        let p_size = header.payload_size;
        crate::log_info!(
            "PILLSMOD",
            "Successful parse: metadata ({} bytes), payload ({} bytes)",
            m_size, p_size
        );

        Ok(Self {
            header,
            metadata,
            payload,
        })
    }

    /// Load and boot the PillsMod driver if it represents an EL1 Kext.
    pub fn load_kext(&self) -> Result<()> {
        // Verification step: verify class = "PillsMod" in metadata
        if !self.metadata.contains("class = \"PillsMod\"") && !self.metadata.contains("class=\"PillsMod\"") {
            crate::log_error!("PILLSMOD", "Cannot load PillsAddon as an EL1 kernel extension (PillsMod)");
            return Err(Status::NotAllowed);
        }

        crate::log_info!("PILLSMOD", "Deploying PillsMod (Kext) payload at EL1...");
        
        // S10 loader step stub: in later steps, we will perform relocation of `self.payload`
        // and jump to the entry symbol. For 1.0-beta, we log the ready status.
        crate::log_info!("PILLSMOD", "[SUCCESS] Kext payload loaded and verified. Ready to link!");

        Ok(())
    }
}

/// Dynamic loading entry called from `kernel_main` at boot time.
pub fn load_all_from_bootloader(table_pa: usize) -> Result<()> {
    if table_pa == 0 {
        return Ok(());
    }
    let table_va = crate::arch::mmu_facade::pa_to_kernel_va(table_pa);
    let table_ptr = table_va as *const DriverTableHandoff;
    let table = unsafe { core::ptr::read_unaligned(table_ptr) };
    let drv_count = table.count;

    crate::log_info!("PILLSMOD", "Received Driver Handoff Table at physical {:#x} ({} drivers)", table_pa, drv_count);

    for i in 0..drv_count as usize {
        if i >= 16 {
            break;
        }
        let drv = &table.drivers[i];
        let drv_name = core::str::from_utf8(&drv.name)
            .unwrap_or("unknown")
            .trim_matches('\0');

        let drv_paddr = drv.paddr;
        let drv_size = drv.size;
        crate::log_info!("PILLSMOD", "[{}] Loading driver package '{}' from physical {:#x}...", i, drv_name, drv_paddr);

        if drv_paddr != 0 && drv_size > 0 {
            // First, flush the dirty data cache lines from the address where the bootloader wrote the payload
            let old_va = crate::arch::mmu_facade::pa_to_kernel_va(drv_paddr as usize);
            crate::arch::mmu::sync_instruction_cache(old_va, drv_size as usize);

            // Allocate a safe, distinct kernel-space virtual address base for this driver module
            // e.g. starting at 0xFFFF_8000_7000_0000 and giving each driver 1MB of space
            let drv_va_base = 0xFFFF_8000_7000_0000usize + i * 0x10_0000;
            let payload_pages = (drv_size as usize + 4095) / 4096;
            let num_pages = payload_pages + 2; // Map 2 extra pages to safely cover uninitialized .bss

            // Map each physical page to the new virtual address region with Kernel RWX permissions
            let mut map_flags = crate::arch::mmu::MapFlags::kernel_rw();
            map_flags.executable = true;

            for page_idx in 0..num_pages {
                let va = drv_va_base + page_idx * 4096;
                let pa = if page_idx < payload_pages {
                    (drv_paddr as usize) + page_idx * 4096
                } else {
                    match crate::arch::aarch64::phys::alloc_page(crate::arch::aarch64::phys::PageTag::KernelHeap) {
                        Ok(pa) => pa.as_usize(),
                        Err(_) => (drv_paddr as usize) + page_idx * 4096, // Fallback contiguous page
                    }
                };

                if let Err(e) = crate::arch::mmu::map_page(va, pa, map_flags) {
                    crate::log_error!("PILLSMOD", "  Failed to map page! va={:#x}, pa={:#x}, err={:?}", va, pa, e);
                } else {
                    crate::log_info!("PILLSMOD", "  Mapped page: va={:#x} -> pa={:#x} flags: writable={:?} executable={:?}", va, pa, map_flags.writable, map_flags.executable);
                }
            }

            // Flush the instruction cache to make sure the CPU fetches the newly mapped instructions
            crate::arch::mmu::sync_instruction_cache(drv_va_base, drv_size as usize);
            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::flush_tlb();
                crate::arch::CurrentArch::invalidate_instruction_cache();
                crate::arch::CurrentArch::memory_barrier();
                crate::arch::CurrentArch::instruction_barrier();
            }

            // Direct diagnostic read of the mapped virtual memory
            let inst_ptr = drv_va_base as *const u32;
            let first_inst = unsafe { core::ptr::read_volatile(inst_ptr) };
            crate::log_info!("PILLSMOD", "  [DIAGNOSTIC] Read first instruction at {:#x}: {:#x}", drv_va_base, first_inst);

            crate::log_info!("PILLSMOD", "  Deploying PillsMod (Kext) payload at EL1 for driver '{}' mapped to VA {:#x}...", drv_name, drv_va_base);
            
            // Pass the active physical function pointers which are guaranteed to be executable and mapped
            let active_log_write = unsafe { core::mem::transmute(kernel_log_write as usize) };
            let active_alloc_pages = unsafe { core::mem::transmute(kernel_alloc_pages as usize) };
            let active_free_pages = unsafe { core::mem::transmute(kernel_free_pages as usize) };

            let relocated_table = libpillsmod::KernelImportTable {
                log_write: active_log_write,
                alloc_pages: active_alloc_pages,
                free_pages: active_free_pages,
                clean_invalidate_cache: unsafe { core::mem::transmute(kernel_clean_invalidate_cache as usize) },
                register_block_device: unsafe { core::mem::transmute(kernel_register_block_device as usize) },
                register_net_device: unsafe { core::mem::transmute(kernel_register_net_device as usize) },
            };

            crate::log_info!("PILLSMOD", "  Active import table (stack): {:#x}", &relocated_table as *const _ as usize);
            crate::log_info!("PILLSMOD", "  Active import fields: log_write={:#x}, alloc_pages={:#x}", relocated_table.log_write as usize, relocated_table.alloc_pages as usize);
            
            // Execute the entry function of the PillsMod driver, passing the KernelImportTable reference
            let entry_fn: extern "C" fn(&libpillsmod::KernelImportTable) -> i32 = unsafe { core::mem::transmute(drv_va_base) };
            let ret = entry_fn(&relocated_table);
            
            crate::log_info!("PILLSMOD", "  [SUCCESS] Kext payload loaded. called pillsmod_init, returned: {}", ret);
        }
    }

    Ok(())
}

extern "C" fn kernel_log_write(tag_ptr: *const u8, tag_len: usize, msg_ptr: *const u8, msg_len: usize) {
    // 1. Direct console output test at the very beginning of the callback
    use core::fmt::Write;
    let mut w = crate::kcore::logging::ConsoleWriter;
    let _ = w.write_str("\n[CALLBACK ENTERED] tag_ptr=");
    // Print tag_ptr as hex
    let mut hex_buf = [0u8; 16];
    let mut val = tag_ptr as usize;
    for i in (0..16).rev() {
        let digit = (val & 0xf) as u8;
        hex_buf[i] = if digit < 10 { b'0' + digit } else { b'a' + digit - 10 };
        val >>= 4;
    }
    let _ = w.write_str(unsafe { core::str::from_utf8_unchecked(&hex_buf) });
    let _ = w.write_str(" msg_ptr=");
    let mut val2 = msg_ptr as usize;
    for i in (0..16).rev() {
        let digit = (val2 & 0xf) as u8;
        hex_buf[i] = if digit < 10 { b'0' + digit } else { b'a' + digit - 10 };
        val2 >>= 4;
    }
    let _ = w.write_str(unsafe { core::str::from_utf8_unchecked(&hex_buf) });
    let _ = w.write_str("\n");

    let tag = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(tag_ptr, tag_len))
            .unwrap_or("PILL")
    };
    let msg = unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(msg_ptr, msg_len))
            .unwrap_or("")
    };
    crate::kcore::logbuf::log_to_ring(
        crate::kcore::logging::LEVEL_INFO,
        tag,
        msg.trim_end_matches('\n'),
    );

    let mut w2 = crate::kcore::logging::ConsoleWriter;
    let _ = w2.write_str("\x1b[1;35mPILL \x1b[0m | \x1b[36m");
    let _ = w2.write_str(tag);
    let _ = w2.write_str(" | ");
    let _ = w2.write_str(msg.trim_end_matches('\n'));
    let _ = w2.write_str("\x1b[0m\n");
}

extern "C" fn kernel_alloc_pages(num_pages: usize) -> u64 {
    if num_pages == 0 {
        return 0;
    }
    let mut first_pa = 0;
    for i in 0..num_pages {
        match crate::arch::aarch64::phys::alloc_page(crate::arch::aarch64::phys::PageTag::KernelHeap) {
            Ok(pa) => {
                if i == 0 {
                    first_pa = pa.as_usize();
                } else if pa.as_usize() != first_pa + i * 4096 {
                    // Contiguity violation: free allocated pages and return 0
                    for j in 0..i {
                        let _ = crate::arch::aarch64::phys::free_page(crate::arch::aarch64::phys::PhysAddr::new(first_pa + j * 4096));
                    }
                    return 0;
                }
            }
            Err(_) => {
                // Out of memory: free allocated pages and return 0
                for j in 0..i {
                    let _ = crate::arch::aarch64::phys::free_page(crate::arch::aarch64::phys::PhysAddr::new(first_pa + j * 4096));
                }
                return 0;
            }
        }
    }
    first_pa as u64
}

extern "C" fn kernel_free_pages(paddr: u64, num_pages: usize) -> i32 {
    let mut success_count = 0;
    for i in 0..num_pages {
        let pa = paddr as usize + i * 4096;
        let status = crate::arch::aarch64::phys::free_page(crate::arch::aarch64::phys::PhysAddr::new(pa));
        if status.is_ok() {
            success_count += 1;
        }
    }
    if success_count == num_pages {
        0
    } else {
        -1
    }
}

extern "C" fn kernel_clean_invalidate_cache(kva: usize, len: usize) {
    unsafe {
        use crate::arch::ArchHardware;
        crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, len);
    }
}

extern "C" fn kernel_register_block_device(ops_ptr: *const libpillsmod::BlockDeviceOps) -> i32 {
    if ops_ptr.is_null() {
        return -1;
    }
    let ops = unsafe { &*ops_ptr };
    
    struct PillsBlockDriverWrapper {
        ops: &'static libpillsmod::BlockDeviceOps,
    }

    impl crate::drivers::block::BlockDriver for PillsBlockDriverWrapper {
        fn read_sectors(&self, sector: u64, dst_pa: usize) -> shared::status::Result<()> {
            let kva = crate::arch::mmu_facade::pa_to_kernel_va(dst_pa);
            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, 512);
            }

            let ret = unsafe { (self.ops.read_sectors)(sector, dst_pa) };

            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, 512);
            }

            if ret == 0 {
                Ok(())
            } else {
                Err(shared::status::Status::from_raw(ret))
            }
        }

        fn write_sectors(&self, sector: u64, src_pa: usize) -> shared::status::Result<()> {
            let kva = crate::arch::mmu_facade::pa_to_kernel_va(src_pa);
            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, 512);
            }

            let ret = unsafe { (self.ops.write_sectors)(sector, src_pa) };

            if ret == 0 {
                Ok(())
            } else {
                Err(shared::status::Status::from_raw(ret))
            }
        }

        fn get_capacity(&self) -> u64 {
            unsafe { (self.ops.get_capacity)() }
        }
    }

    let wrapper = PillsBlockDriverWrapper { ops };
    let boxed_wrapper = alloc::boxed::Box::new(wrapper);
    let leaked_wrapper: &'static PillsBlockDriverWrapper = alloc::boxed::Box::leak(boxed_wrapper);
    
    *crate::drivers::block::ACTIVE_BLOCK_DEVICE.lock() = Some(leaked_wrapper);
    crate::log_info!("PILLSMOD", "Dynamic block driver registered successfully with Kernel ACTIVE_BLOCK_DEVICE!");
    0
}

extern "C" fn kernel_register_net_device(ops_ptr: *const libpillsmod::NetDeviceOps) -> i32 {
    if ops_ptr.is_null() {
        return -1;
    }
    let ops = unsafe { &*ops_ptr };

    struct PillsNetDriverWrapper {
        ops: &'static libpillsmod::NetDeviceOps,
    }

    impl crate::drivers::net::NetDriver for PillsNetDriverWrapper {
        fn send_packet(&self, buf: &[u8]) -> shared::status::Result<()> {
            let buf_pa = (buf.as_ptr() as usize).wrapping_sub(crate::arch::mmu_facade::KERNEL_OFFSET);
            let kva = buf.as_ptr() as usize;
            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, buf.len());
            }

            let ret = unsafe { (self.ops.send_packet)(buf_pa, buf.len()) };

            if ret == 0 {
                Ok(())
            } else {
                Err(shared::status::Status::from_raw(ret))
            }
        }

        fn recv_packet(&self, buf: &mut [u8]) -> shared::status::Result<usize> {
            let buf_pa = (buf.as_ptr() as usize).wrapping_sub(crate::arch::mmu_facade::KERNEL_OFFSET);
            let kva = buf.as_ptr() as usize;
            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, buf.len());
            }

            let ret = unsafe { (self.ops.recv_packet)(buf_pa, buf.len()) };

            unsafe {
                use crate::arch::ArchHardware;
                crate::arch::CurrentArch::clean_and_invalidate_cache_range(kva, buf.len());
            }

            if ret >= 0 {
                Ok(ret as usize)
            } else {
                Err(shared::status::Status::from_raw(ret))
            }
        }
    }

    let wrapper = PillsNetDriverWrapper { ops };
    let boxed_wrapper = alloc::boxed::Box::new(wrapper);
    let leaked_wrapper: &'static PillsNetDriverWrapper = alloc::boxed::Box::leak(boxed_wrapper);

    *crate::drivers::net::ACTIVE_NET_DEVICE.lock() = Some(leaked_wrapper);
    crate::log_info!("PILLSMOD", "Dynamic network driver registered successfully with Kernel ACTIVE_NET_DEVICE!");
    0
}

static KERNEL_IMPORT_TABLE: libpillsmod::KernelImportTable = libpillsmod::KernelImportTable {
    log_write: kernel_log_write,
    alloc_pages: kernel_alloc_pages,
    free_pages: kernel_free_pages,
    clean_invalidate_cache: kernel_clean_invalidate_cache,
    register_block_device: kernel_register_block_device,
    register_net_device: kernel_register_net_device,
};
