use crate::color::Color;

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub buffer: &'static mut [u8],
}

impl Canvas {
    pub fn new(width: u32, height: u32, buffer: &'static mut [u8]) -> Self {
        Canvas { width, height, buffer }
    }
    pub fn clear(&mut self, color: Color) {
        for pixel in self.buffer.chunks_mut(4) {
            pixel[0] = color.b;
            pixel[1] = color.g;
            pixel[2] = color.r;
            pixel[3] = color.a;
        }
    }
    pub fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.width || y >= self.height { return; }
        let offset = ((y * self.width + x) * 4) as usize;
        if offset + 3 < self.buffer.len() {
            self.buffer[offset] = color.b;
            self.buffer[offset + 1] = color.g;
            self.buffer[offset + 2] = color.r;
            self.buffer[offset + 3] = color.a;
        }
    }
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        for dy in 0..h { for dx in 0..w { self.set_pixel(x + dx, y + dy, color); } }
    }
}
