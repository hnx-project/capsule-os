pub struct Font;

impl Font {
    pub fn new() -> Self { Font }
    pub fn draw_char(&self, canvas: &mut crate::canvas::Canvas, c: char, x: u32, y: u32, color: crate::color::Color) {
        let _ = (canvas, c, x, y, color);
    }
    pub fn draw_text(&self, canvas: &mut crate::canvas::Canvas, text: &str, x: u32, y: u32, color: crate::color::Color) {
        let mut cx = x;
        for c in text.chars() {
            self.draw_char(canvas, c, cx, y, color);
            cx += 8;
        }
    }
}
