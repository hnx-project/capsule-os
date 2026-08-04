#![no_std]
#![no_main]

extern crate shared;

use pillskit::{PillsBus, pill_log_info, pill_log_error};

#[no_mangle]
pub fn main() -> i32 {
    #[cfg(target_env = "capsule")]
    {
        pill_log_info!("MOCK-DRIVER", "Starting PILLADDON (EL0 User-Space Sandbox) Driver...");
    }
    #[cfg(not(target_env = "capsule"))]
    {
        pill_log_info!("MOCK-DRIVER", "Starting PILLMOD (EL1 Kernel-Space Native) Driver...");
    }

    // Attempt to probe a mock MMIO register (e.g. VirtIO MMIO slot 0 magic number)
    let magic = PillsBus::mmio_read(0x0a000000, 0x000);
    pill_log_info!("MOCK-DRIVER", "MMIO read at 0x0a000000 offset 0x000: magic={:#x}", magic);

    // Yield CPU to showcase scheduler interaction
    for i in 1..=3 {
        pill_log_info!("MOCK-DRIVER", "Processing event queue tick {}...", i);
        PillsBus::yield_cpu();
    }

    pill_log_info!("MOCK-DRIVER", "Pillskit Dual-Mode Mock Driver shut down safely.");
    0
}
