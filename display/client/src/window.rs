#[derive(Debug, Clone, Copy)]
pub struct ClientWindow {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

impl ClientWindow {
    pub fn new(id: u32, width: u32, height: u32) -> Self {
        ClientWindow { id, width, height }
    }
}
