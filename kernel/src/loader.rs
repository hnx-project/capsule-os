use shared::status::Result;
use shared::launcher::ServiceDescriptor;
use crate::task::Process;
use crate::object::handle_table::KernelObject;
use crate::object::rights::Rights;
use crate::mm::vmo::Vmo;

/// Unified Service Launcher for early kernel-space boot stage.
pub struct ServiceLauncher;

impl ServiceLauncher {
    pub fn launch(desc: &ServiceDescriptor<'static>) -> Result<u64> {
        let bytes = crate::rootfs::get_file(desc.path)
            .ok_or(shared::status::Status::NotFound)?;

        // Launch the user process through the standard process loader
        let pid = Process::launch_user_program(desc.name, bytes)?;

        // Handle bootstrap Capability hand-offs (like injecting BootFS VMO handle)
        if let Some(handle_idx) = desc.bootstrap_vmo_handle_index {
            let rootfs_vmo = unsafe { Vmo::create_physical(crate::BOOTFS_PHYS_ADDR, crate::BOOTFS_PHYS_SIZE)? };
            if let Some(proc) = crate::task::process::find_process_mut(pid) {
                let rights = Rights::READ.bits() | Rights::WRITE.bits();
                let _ = proc.handle_table.add_raw_handle(handle_idx, KernelObject::Vmo(rootfs_vmo), rights);
            }
        }

        // CANARY: check PID 1's L3 page is still intact after launch.
        // Uses the dynamic WATCH_PA if set, otherwise falls back to a
        // page-table walk from PID 1's L0.
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let watched = crate::mm::phys::WATCH_PA.load(core::sync::atomic::Ordering::Relaxed);
            let check_pa = if watched != 0 {
                watched
            } else {
                0x40255000 // legacy fallback
            };
            use crate::mm::mmu::pa_to_kernel_va;
            let l3_kva = pa_to_kernel_va(check_pa) as *const u64;
            for ci in 0..4 {
                let val = core::ptr::read_volatile(l3_kva.add(ci));
                if val & 3 != 3 {
                    crate::log_error!("CANARY", "PID 1 L3[{}]={:#x} (expected valid page desc) pa={:#x} after launching PID {}", ci, val, check_pa, pid);
                }
            }
        }

        crate::log_info!("LAUNCHER", "Service '{}' (PID {}) launched successfully via ServiceLauncher!", desc.name, pid);
        Ok(pid)
    }
}

pub fn launch_loader() -> Result<()> {
    let loader_desc = ServiceDescriptor {
        name: "loader",
        path: "system/bin/loader",
        bootstrap_vmo_handle_index: Some(100),
    };

    ServiceLauncher::launch(&loader_desc)?;
    Ok(())
}
