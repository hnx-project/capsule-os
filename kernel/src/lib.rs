#![no_std]

extern crate hal;
extern crate shared;

mod arch;
mod task;
mod mm;
mod ipc;
mod object;
mod syscall;
mod sync;
mod kcore;

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
