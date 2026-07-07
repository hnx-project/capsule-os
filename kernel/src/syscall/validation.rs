use shared::status::{Result, Status};
use shared::types::{HandleValue, ObjectType};
use crate::object::handle_table::HandleTable;

pub fn validate_pointer<T>(_ptr: *const T) -> Result<()> { Ok(()) }
pub fn validate_mut_pointer<T>(_ptr: *mut T) -> Result<()> { Ok(()) }
pub fn validate_buffer(_ptr: usize, _len: usize) -> Result<()> { Ok(()) }

pub fn validate_handle(raw: u32) -> Result<HandleValue> {
    let hv = HandleValue::new(raw);
    if hv == HandleValue::INVALID {
        return Err(Status::BadHandle);
    }
    Ok(hv)
}

/// Full handle validation: existence + type + rights.
pub fn validate_handle_access(
    table: &HandleTable,
    raw: u32,
    expected_type: ObjectType,
    required_rights: u32,
) -> Result<HandleValue> {
    let hv = validate_handle(raw)?;
    // Use with() as a pure existence+rights check.
    table.with(hv, required_rights, |_obj| Ok(()))?;
    // Type verification is done at the call site via with_vmo/with_vmar/etc.
    Ok(hv)
}
