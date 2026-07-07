pub trait Console {
    fn putchar(&mut self, c: u8);
    fn getchar(&mut self) -> Option<u8>;
    fn flush(&mut self);
}
