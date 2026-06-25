#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub type_: EventType,
    pub window_id: u32,
    pub x: i32,
    pub y: i32,
    pub key: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    MouseMove,
    MousePress,
    MouseRelease,
    KeyPress,
    KeyRelease,
    Expose,
    Close,
}
