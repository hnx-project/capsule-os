#![no_std]
#![no_main]

extern crate hnxlibc;

fn print(s: &str) {
    unsafe {
        hnxlibc::write(1, s.as_ptr(), s.len());
    }
}

fn println(s: &str) {
    print(s);
    print("\n");
}

#[no_mangle]
pub fn main() -> i32 {
    println("init: starting...");
    println("init: handing off to osh");
    hnxlibc::exec("osh");
    0
}
