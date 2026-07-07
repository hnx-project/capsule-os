#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct HandleValue(pub u32);

impl HandleValue {
    pub const INVALID: HandleValue = HandleValue(0);
    pub const fn new(val: u32) -> Self {
        HandleValue(val)
    }
    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ObjectType {
    Process = 0,
    Thread = 1,
    Vmo = 2,
    Vmar = 3,
    Channel = 4,
    Port = 5,
    Event = 6,
    Timer = 7,
    UserThread = 8,
    Job = 9,
    VmObject = 10,
    Unknown = 0xFF,
}

impl ObjectType {
    pub fn from_u32(val: u32) -> Self {
        match val {
            0 => ObjectType::Process,
            1 => ObjectType::Thread,
            2 => ObjectType::Vmo,
            3 => ObjectType::Vmar,
            4 => ObjectType::Channel,
            5 => ObjectType::Port,
            6 => ObjectType::Event,
            7 => ObjectType::Timer,
            8 => ObjectType::UserThread,
            9 => ObjectType::Job,
            10 => ObjectType::VmObject,
            _ => ObjectType::Unknown,
        }
    }
    pub fn to_u32(self) -> u32 {
        self as u32
    }
}
