#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self { Color { r, g, b, a } }
    pub fn rgb(r: u8, g: u8, b: u8) -> Self { Color { r, g, b, a: 255 } }
    pub fn BLACK() -> Self { Self::rgb(0, 0, 0) }
    pub fn WHITE() -> Self { Self::rgb(255, 255, 255) }
    pub fn RED() -> Self { Self::rgb(255, 0, 0) }
    pub fn GREEN() -> Self { Self::rgb(0, 255, 0) }
    pub fn BLUE() -> Self { Self::rgb(0, 0, 255) }
}
