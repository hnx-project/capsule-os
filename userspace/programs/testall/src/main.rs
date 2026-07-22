#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

extern crate libc;
extern crate libcapsule;

mod dev;
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

    // Device manager tests (direct svc.dev IPC)
    t.run("dev_connect", dev::test_dev_connect());
    t.run("dev_probe", dev::test_dev_probe());
    t.run("dev_list", dev::test_dev_list());
    t.run("dev_info_pl011", dev::test_dev_info_pl011());
    t.run("dev_open_close", dev::test_dev_open_close());
    t.run("dev_open_nonexist", dev::test_dev_open_nonexist());
    t.run("dev_read_uart", dev::test_dev_read_uart());
    t.run("dev_write_uart", dev::test_dev_write_uart());

    // VFS filesystem tests (via POSIX libc → fileagent svc.vfs IPC)
    t.run("connect", vfs::test_connect());
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

    // FatFS block storage stack VFS tests
    t.run("boot_create_file", vfs::test_boot_create_file());
    t.run("boot_read_file", vfs::test_boot_read_file());
    t.run("boot_mkdir", vfs::test_boot_mkdir());
    t.run("boot_unlink", vfs::test_boot_unlink());

    // POSIX device tests (posix_open → fileagent → devmgr)
    t.run("dev_open_pl011", vfs::test_dev_open_pl011());
    t.run("dev_open_nonexist2", vfs::test_dev_open_nonexist());
    t.run("dev_read_pl011", vfs::test_dev_read_pl011());
    t.run("dev_write_pl011", vfs::test_dev_write_pl011());

    if t.total > 0 {
        kprintln!("{}/{} passed", t.passed, t.total);
    } else {
        kprintln!("No tests needed");
    }

    if t.passed == t.total {
        0
    } else {
        1
    }
}
