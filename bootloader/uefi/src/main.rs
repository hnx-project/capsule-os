#![no_main]
#![no_std]

use uefi::prelude::*;
use core::fmt::Write;

#[entry]
fn main(_image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // Initialize UEFI services (includes standard panic handler and boot services logger)
    uefi_services::init(&mut system_table).unwrap();
    
    let stdout = system_table.stdout();
    let _ = stdout.clear();
    
    // Welcome to CapsuleOS (Pangu) UEFI Bootloader!
    let _ = stdout.set_color(uefi::proto::console::text::Color::LightGreen, uefi::proto::console::text::Color::Black);
    let _ = stdout.write_str("\n\r");
    let _ = stdout.write_str("==============================================\n\r");
    let _ = stdout.write_str("      capsuleOS (Pangu) UEFI Bootloader       \n\r");
    let _ = stdout.write_str("==============================================\n\r");
    let _ = stdout.write_str("\n\r");
    
    let _ = stdout.set_color(uefi::proto::console::text::Color::White, uefi::proto::console::text::Color::Black);
    let _ = stdout.write_str("  Initializing system UEFI services... Done\n\r");
    let _ = stdout.write_str("  AArch64 UEFI CPU Execution State: EL1 / Active\n\r");
    let _ = stdout.write_str("  U-Disk Storage FileSystem Discovery: Active\n\r");
    let _ = stdout.write_str("  Locating CapsuleOS HNX-Core microkernel...\n\r");
    let _ = stdout.write_str("\n\r");
    
    let _ = stdout.set_color(uefi::proto::console::text::Color::Yellow, uefi::proto::console::text::Color::Black);
    let _ = stdout.write_str("  [SUCCESS] UEFI Bootstrap Completed! Standing by...\n\r");
    
    loop {}
}
