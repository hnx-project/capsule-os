#[derive(Debug, Clone, Copy)]
pub enum ClientEvent {
    Created { window_id: u32 },
    Draw { window_id: u32 },
    MousePress { window_id: u32, x: i32, y: i32, button: u8 },
    MouseRelease { window_id: u32, x: i32, y: i32, button: u8 },
    KeyPress { window_id: u32, key: u32 },
    KeyRelease { window_id: u32, key: u32 },
    Close { window_id: u32 },
}
