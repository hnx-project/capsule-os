#![no_std]
#![crate_type = "staticlib"]

extern crate hal;
extern crate shared;

pub mod arch;
pub mod task;
pub mod mm;
pub mod ipc;
pub mod object;
pub mod syscall;
pub mod sync;
pub mod kcore;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() {
    arch::early_init();
    kcore::init();
    mm::init();

    let scheduler = task::scheduler::Scheduler::new();
    let init_thread = task::create_init_thread();
    scheduler.add(init_thread);

    scheduler.run();

    loop {}
}
