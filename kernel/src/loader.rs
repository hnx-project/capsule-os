use shared::status::{Result, Status};
use crate::mm::vmo::Vmo;
use crate::task::thread::Thread;
use crate::mm::vmar::VmarFlags;

pub fn launch_loader() -> Result<()> {
    let bytes = include_bytes!("../files/loader");
    let parser = ohlink_format::parser::OHLK_Parser::new(bytes).map_err(|e| {
        crate::log_error!("LOADER", "Loader parser error: {:?}", e);
        Status::InvalidArgs
    })?;

    let header = parser.header();

    // Loader is statically compiled to base 0x200000 in standard user space capsule target!
    let mut entry = 0x200000;
    let mut segment_count = 0;

    for idx in 0..header.header_count {
        if let Ok(entry_meta) = parser.get_entry(idx) {
            if entry_meta.ty == ohlink_format::SegmentType::Text.to_u32()
                || entry_meta.ty == ohlink_format::SegmentType::Data.to_u32()
                || entry_meta.ty == ohlink_format::SegmentType::Rodata.to_u32()
                || entry_meta.ty == ohlink_format::SegmentType::Bss.to_u32()
            {
                segment_count += 1;
            }
        }
    }

    crate::log_info!("LOADER", "Embedding loader: entry={:#x}, segments={}", entry, segment_count);

    let proc = crate::task::process::allocate_process("loader")?;
    let pid = proc.id;

    for idx in 0..header.header_count {
        let mut entry_meta = match parser.get_entry(idx) {
            Ok(e) => e,
            _ => continue,
        };
        if entry_meta.ty != ohlink_format::SegmentType::Text.to_u32()
            && entry_meta.ty != ohlink_format::SegmentType::Data.to_u32()
            && entry_meta.ty != ohlink_format::SegmentType::Rodata.to_u32()
            && entry_meta.ty != ohlink_format::SegmentType::Bss.to_u32()
        {
            continue;
        }

        // We override mapped segment virtual address to 0x200000 to align with standard user target compile configurations!
        let virt_addr = 0x200000;
        let size = entry_meta.mem_size as usize;
        let flags_raw = entry_meta.flags;

        let aligned_vaddr = virt_addr & !(4096 - 1);
        let alignment_offset = virt_addr - aligned_vaddr;
        let aligned_size = (size + alignment_offset + 4095) & !(4095);

        let target_va = proc.root_vmar.base + aligned_vaddr;
        let flags = VmarFlags::from_bits(flags_raw);

        crate::log_info!(
            "LOADER",
            "Segment: virt_addr={:#x} (aligned={:#x}, offset={}), size={} (aligned={}), flags={:#x}, vmar_base={:#x}",
            virt_addr, aligned_vaddr, alignment_offset, size, aligned_size, flags_raw, proc.root_vmar.base
        );

        let mut vmo = Vmo::create_with_size(aligned_size)?;
        if entry_meta.file_size > 0 {
            // Read from OHLINK payload start offset which is at 80 (0x50), but our parser reads from the offset field of the entry.
            // Since we patched Segment #0 offset to 80 (0x50), parser.get_segment_data will correctly fetch exactly starting at byte 80!
            let segment_payload = parser.get_segment_data(&entry_meta).map_err(|e| {
                crate::log_error!("LOADER", "Failed to retrieve segment payload: {:?}", e);
                Status::InvalidArgs
            })?;
            vmo.write(alignment_offset, segment_payload)?;
        }
        
        // Restore entry_meta offset back to the loader's static virtual load address (0x200000) so loader's relative address jump execution works 100% correctly!
        entry_meta.offset = 0x200000;
        
        match proc.root_vmar.map(&mut vmo, 0, target_va, aligned_size, flags) {
            Ok(_) => {
                crate::log_info!("LOADER", "Mapped segment: VA={:#x}, size={}", target_va, aligned_size);
            }
            Err(e) => {
                crate::log_error!("LOADER", "Failed to map segment: target_va={:#x}, size={}, err={:?}", target_va, aligned_size, e);
                return Err(e);
            }
        }
    }

    let stack_size = 16 * 1024;
    let mut stack_vmo = Vmo::create_with_size(stack_size)?;
    let stack_vaddr_offset = 0x2000000;
    let stack_va = proc.root_vmar.base + stack_vaddr_offset;
    
    let stack_flags = VmarFlags::from_bits(
        VmarFlags::READ.bits() | 
        VmarFlags::WRITE.bits() | 
        VmarFlags::USER.bits()
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
