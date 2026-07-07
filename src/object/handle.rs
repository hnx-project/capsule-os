use shared::status::Result;
use shared::status::Status;
use shared::types::{HandleValue, ObjectType};

#[derive(Debug)]
pub struct Handle {
    pub value: HandleValue,
    pub object_type: ObjectType,
    pub rights: u32,
    pub object: *mut (),
}

impl Handle {
    pub fn new(object: *mut (), object_type: ObjectType, rights: u32) -> Self {
        Handle {
            value: HandleValue::INVALID,
            object_type,
            rights,
            object,
        }
    }

    pub fn check_rights(&self, required: u32) -> bool {
        (self.rights & required) == required
    }

    pub fn check_type(&self, expected: ObjectType) -> Result<()> {
        if self.object_type == expected {
            Ok(())
        } else {
            Err(Status::WrongType)
        }
    }
}
