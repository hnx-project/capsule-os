#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub title: &'static str,
    pub visible: bool,
}

impl Window {
    pub fn new(id: u32, title: &'static str) -> Self {
        Window { id, x: 100, y: 100, width: 400, height: 300, title, visible: true }
    }
    pub fn move_to(&mut self, x: i32, y: i32) { self.x = x; self.y = y; }
    pub fn resize(&mut self, width: u32, height: u32) { self.width = width; self.height = height; }
    pub fn show(&mut self) { self.visible = true; }
    pub fn hide(&mut self) { self.visible = false; }
}
