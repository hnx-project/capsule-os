#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // QEMU PL011 UART virtual serial port address on Virt ARM64 machine
    let uart = 0x09000000 as *mut u8;
    
    // Write "Hello HNX\n" to the serial port
    for &byte in b"Hello HNX\n" {
        unsafe {
            *uart = byte;
        }
    }
    
    loop {}
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
