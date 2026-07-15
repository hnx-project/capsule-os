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
