use shared::types::{HandleValue, ObjectType};

#[derive(Debug)]
pub struct Handle {
    pub value: HandleValue,
    pub object_type: ObjectType,
    pub rights: u32,
}

impl Handle {
    pub fn new(object_type: ObjectType, rights: u32) -> Self {
        Handle { value: HandleValue::INVALID, object_type, rights }
    }
    pub fn check_rights(&self, required: u32) -> bool {
        (self.rights & required) == required
    }
}
