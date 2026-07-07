use core::ops::BitOr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rights(u32);

impl Rights {
    pub const NONE: Rights = Rights(0);
    pub const READ: Rights = Rights(1 << 0);
    pub const WRITE: Rights = Rights(1 << 1);
    pub const EXECUTE: Rights = Rights(1 << 2);
    pub const MAP: Rights = Rights(1 << 3);
    pub const DUPLICATE: Rights = Rights(1 << 4);
    pub const TRANSFER: Rights = Rights(1 << 5);
    pub const GET_PROPERTY: Rights = Rights(1 << 6);
    pub const SET_PROPERTY: Rights = Rights(1 << 7);
    pub const ENUMERATE: Rights = Rights(1 << 8);
    pub const SIGNAL: Rights = Rights(1 << 9);

    pub const fn from_bits(b: u32) -> Self { Rights(b) }
    pub const fn bits(self) -> u32 { self.0 }

    pub fn contains(self, other: Rights) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl BitOr for Rights {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Rights(self.0 | rhs.0)
    }
}
