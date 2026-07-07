use shared::status::{Result, Status};
use shared::types::HandleValue;
use crate::mm::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;

pub fn sys_vmo_create(table: &HandleTable, size: usize) -> Result<HandleValue> {
    let vmo = Vmo::create_with_size(size)?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    table.add(KernelObject::Vmo(vmo), rights)
}

pub fn sys_vmo_read(table: &HandleTable, handle_raw: u32, offset: usize,
                    buf: &mut [u8]) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    table.with_vmo(hv, Rights::READ.bits(), move |vmo| {
        vmo.read(offset, buf).unwrap_or(0)
    })
}

pub fn sys_vmo_write(table: &HandleTable, handle_raw: u32, offset: usize,
                     buf: &[u8]) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    table.with_vmo(hv, Rights::WRITE.bits(), move |vmo| {
        vmo.write(offset, buf).unwrap_or(0)
    })
}

pub fn sys_vmar_map(
    table: &HandleTable,
    process_handle_raw: u32,
    vmo_handle_raw: u32,
    vmo_offset: usize,
    size: usize,
    vaddr_offset: usize,
    flags_raw: u32,
) -> Result<usize> {
    let p_hv = HandleValue::new(process_handle_raw);
    let pid = table.with_process(p_hv, Rights::WRITE.bits(), |id| id)?;

    let proc = crate::task::process::find_process_mut(pid).ok_or(Status::NotFound)?;
    let vmo_hv = HandleValue::new(vmo_handle_raw);

    let target_va = proc.root_vmar.base + vaddr_offset;
    let flags = crate::mm::vmar::VmarFlags::from_bits(flags_raw);

    table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| {
        proc.root_vmar.map(vmo, vmo_offset, target_va, size, flags)
    })?
}
