pub fn println(s: &str) {
    use libc::write;
    let _ = write(1, s.as_ptr(), s.len());
    let _ = write(1, b"\n".as_ptr(), 1);
}

pub fn print(s: &str) {
    use libc::write;
    let _ = write(1, s.as_ptr(), s.len());
}

pub fn read_line(buf: &mut [u8]) -> isize {
    libc::read(0, buf.as_mut_ptr(), buf.len())
}
