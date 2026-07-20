pub struct Cursor<T> {
    inner: T,
    pos: usize,
}

impl<T> Cursor<T> {
    pub fn new(inner: T) -> Self { Cursor { inner, pos: 0 } }
    pub fn position(&self) -> usize { self.pos }
}
