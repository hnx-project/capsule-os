#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

extern crate libcapsule;

mod vfs;

use libcapsule::kprintln;

struct TestRunner {
    passed: u32,
    total: u32,
}

impl TestRunner {
    fn new() -> Self {
        Self {
            passed: 0,
            total: 0,
        }
    }

    fn run(&mut self, name: &str, ok: bool) {
        self.total += 1;
        if ok {
            self.passed += 1;
            kprintln!("[PASS] {}", name);
        } else {
            kprintln!("[FAIL] {}", name);
        }
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let mut t = TestRunner::new();

    kprintln!("===== All Test Suite =====");

    // Standard POSIX Connect Test: verify we can open/close /dev/tty via standard libc
    let fd = vfs::posix_open("/dev/tty");
    if fd < 0 {
        kprintln!("[FAIL] connect");
        return 1;
    }
    let _ = vfs::posix_close(fd);
    t.run("connect", true);

    t.run("create_file", vfs::test_create_file());
    t.run("read_file", vfs::test_read_file());
    t.run("mkdir", vfs::test_mkdir());
    t.run("mkdir_dup", vfs::test_mkdir_dup());
    t.run("readdir", vfs::test_readdir());
    t.run("rmdir", vfs::test_rmdir());
    t.run("unlink", vfs::test_unlink());
    t.run("open_nonexist", vfs::test_open_nonexist());
    t.run("stat_root", vfs::test_stat_root());
    t.run("stat_file", vfs::test_stat_file());
    t.run("tty", vfs::test_tty());

    kprintln!("{}/{} passed", t.passed, t.total);

    if t.passed == t.total {
        0
    } else {
        1
    }
}
