use shared::Status;

pub trait Console: Send + Sync {
    fn putchar(&mut self, c: u8);
    fn getchar(&mut self) -> Option<u8>;
    fn flush(&mut self);
}

pub trait Serial: Console {
    fn init(&mut self) -> Status;
}
