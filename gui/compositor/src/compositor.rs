use crate::window::Window;

pub struct Compositor {
    windows: &'static mut [Option<Window>],
    focused: Option<u32>,
    next_id: u32,
    capacity: usize,
}

impl Compositor {
    pub fn new() -> Self {
        Compositor {
            windows: &mut [],
            focused: None,
            next_id: 1,
            capacity: 0,
        }
    }
    pub fn create_window(&mut self, _title: &'static str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn destroy_window(&mut self, _id: u32) {}
    pub fn focus_window(&mut self, id: u32) { self.focused = Some(id); }
    pub fn get_focused(&self) -> Option<u32> { self.focused }
}
