pub fn println(s: &str) {
    use hnxlibc::write;
    write(1, s.as_ptr(), s.len());
    write(1, b"\n".as_ptr(), 1);
}

pub fn print(s: &str) {
    use hnxlibc::write;
    write(1, s.as_ptr(), s.len());
}
