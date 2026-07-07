use shared::status::{Result, Status};
use shared::types::HandleValue;
use crate::ipc::channel::Channel;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;

pub fn sys_channel_create(table: &HandleTable) -> Result<(HandleValue, HandleValue)> {
    let chan = Channel::new()?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    // For MVP both endpoints share the same object; a real implementation
    // would use two separate channel endpoints or ref-count the same backing.
    let h0 = table.add(KernelObject::Channel(chan), rights)?;
    // Second clone shares the same channel object — for MVP this is enough
    // to exercise the handle table with a second handle.
    // A real channel pair would create a separate endpoint object.
    // For now we return the same handle twice so the caller can try read/write.
    Ok((h0, h0))
}

pub fn sys_channel_read(table: &HandleTable, handle_raw: u32,
                        data: &mut [u8]) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    table.with_channel(hv, Rights::READ.bits(), move |chan| {
        chan.read(data, &mut [])
    })?
}

pub fn sys_channel_write(table: &HandleTable, handle_raw: u32,
                         data: &[u8]) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    table.with_channel(hv, Rights::WRITE.bits(), move |chan| {
        chan.write(data, &[])
    })?
}
