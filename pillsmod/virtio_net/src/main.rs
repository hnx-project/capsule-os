#![no_std]
#![no_main]

use libpillsmod::{pill_print, KernelImportTable, NetDeviceOps};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Helper to translate physical address to high-half kernel virtual address in EL1.
fn pa_to_kernel_va(pa: usize) -> usize {
    pa.wrapping_add(0xFFFF_8000_0000_0000)
}

/// The selected Virtio MMIO network device base address. 0 means not found/uninitialized.
static VIRTIO_NET_BASE: AtomicUsize = AtomicUsize::new(0);

/// Core MAC Address of the discovered network card.
static mut MAC_ADDRESS: [u8; 6] = [0; 6];

/// Static instance of the net device callbacks.
static NET_DEVICE_OPS: NetDeviceOps = NetDeviceOps {
    send_packet,
    recv_packet,
};

#[no_mangle]
#[link_section = ".entry"]
pub extern "C" fn pillsmod_init(kernel: &KernelImportTable) -> i32 {
    pill_print(kernel, "Virtio-Net PillsMod loaded! Probing MMIO bus slot...");

    // Probe 32 MMIO slots starting at 0x0a000000
    for i in 0..32 {
        let base_pa = 0x0a000000 + i * 0x200;
        let base_va = pa_to_kernel_va(base_pa);
        let magic = unsafe { core::ptr::read_volatile(base_va as *const u32) };
        let dev_id = unsafe { core::ptr::read_volatile((base_va + 0x008) as *const u32) };

        if magic == 0x74726976 { // "virt"
            if dev_id == 1 { // Network Device
                pill_print(kernel, "Discovered Virtio-Net device! Initializing...");
                match unsafe { init_device(kernel, base_va) } {
                    Ok(()) => {
                        VIRTIO_NET_BASE.store(base_va, Ordering::SeqCst);
                        let mac = unsafe { MAC_ADDRESS };
                        
                        // Register this driver back to the kernel
                        let ret = (kernel.register_net_device)(&NET_DEVICE_OPS);
                        if ret == 0 {
                            pill_print(kernel, "Virtio-Net driver successfully initialized and registered as ACTIVE_NET_DEVICE!");
                            return 0;
                        } else {
                            pill_print(kernel, "Failed to register network device with kernel!");
                            return -1;
                        }
                    }
                    Err(e) => {
                        pill_print(kernel, "Failed to initialize Virtio-Net device!");
                        return e;
                    }
                }
            }
        }
    }

    pill_print(kernel, "No Virtio-Net device found on MMIO slots.");
    -1
}

#[no_mangle]
pub extern "C" fn pillsmod_exit(_kernel: &KernelImportTable) -> i32 {
    0
}

/// Legacy Virtio MMIO Network Device Initialization.
unsafe fn init_device(_kernel: &KernelImportTable, base: usize) -> Result<(), i32> {
    // 1. Reset device
    core::ptr::write_volatile((base + 0x070) as *mut u32, 0); // Status = 0
    
    // 2. Acknowledge & Driver status bits
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1); // Status = ACKNOWLEDGE
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2); // Status = ACKNOWLEDGE | DRIVER

    // 3. Negotiate features
    core::ptr::write_volatile((base + 0x014) as *mut u32, 0); // DeviceFeaturesSel = 0
    let f0 = core::ptr::read_volatile((base + 0x010) as *const u32);
    
    // Accept features
    core::ptr::write_volatile((base + 0x020) as *mut u32, 0); // DriverFeaturesSel = 0
    core::ptr::write_volatile((base + 0x01c) as *mut u32, f0); 

    // 4. Set DRIVER_OK status bit
    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 4); // Status = ACKNOWLEDGE | DRIVER | FEATURES_OK
    
    // 5. Read MAC address from configuration space (starts at 0x100 for legacy MMIO)
    for i in 0..6 {
        MAC_ADDRESS[i] = core::ptr::read_volatile((base + 0x100 + i) as *const u8);
    }
    
    // If MAC is empty (all zeros), assign a standard default unicast MAC
    if MAC_ADDRESS.iter().all(|&b| b == 0) {
        MAC_ADDRESS = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    }

    core::ptr::write_volatile((base + 0x070) as *mut u32, 1 | 2 | 4 | 8); // Status = ACKNOWLEDGE | DRIVER | FEATURES_OK | DRIVER_OK

    Ok(())
}

extern "C" fn send_packet(_buf_pa: usize, _len: usize) -> i32 {
    let base = VIRTIO_NET_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0; // Fallback loopback route (mock)
    }
    0
}

extern "C" fn recv_packet(_buf_pa: usize, _max_len: usize) -> i32 {
    let base = VIRTIO_NET_BASE.load(Ordering::Relaxed);
    if base == 0 {
        return 0; // Fallback loopback: no packets pending
    }
    0 // Return 0 bytes received
}
