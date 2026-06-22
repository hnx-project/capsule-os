#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    NotFound = -1,
    NoMemory = -2,
    InvalidArgs = -3,
    BadHandle = -4,
    WrongType = -5,
    AccessDenied = -6,
    AlreadyExists = -10,
    NotEmpty = -11,
    FileOpen = -12,
    FileNotFound = -13,
    ProcessNotFound = -20,
    ThreadNotFound = -21,
    InvalidImage = -22,
    PeerClosed = -30,
    NotAllowed = -31,
    TryAgain = -40,
    TimedOut = -41,
    Canceled = -42,
}

impl Status {
    #[inline]
    pub fn to_raw(self) -> usize { self as i32 as usize }
    #[inline]
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Status::Ok, -1 => Status::NotFound, -2 => Status::NoMemory,
            -3 => Status::InvalidArgs, -4 => Status::BadHandle, -5 => Status::WrongType,
            -6 => Status::AccessDenied, -10 => Status::AlreadyExists,
            -11 => Status::NotEmpty, -12 => Status::FileOpen, -13 => Status::FileNotFound,
            -20 => Status::ProcessNotFound, -21 => Status::ThreadNotFound,
            -22 => Status::InvalidImage, -30 => Status::PeerClosed,
            -31 => Status::NotAllowed, -40 => Status::TryAgain,
            -41 => Status::TimedOut, -42 => Status::Canceled,
            _ => Status::InvalidArgs,
        }
    }
    #[inline]
    pub fn is_ok(self) -> bool { self == Status::Ok }
    #[inline]
    pub fn is_err(self) -> bool { !self.is_ok() }
}

pub type Result<T> = core::result::Result<T, Status>;

impl From<Status> for usize {
    fn from(status: Status) -> usize { status.to_raw() }
}
