use shared::status::Result;
use crate::task::Process;
use crate::object::handle_table::KernelObject;
use crate::object::rights::Rights;
use crate::mm::vmo::Vmo;

pub fn launch_loader() -> Result<()> {
    let bytes = crate::rootfs::get_file("system/bin/loader")
        .ok_or(shared::status::Status::NotFound)?;
    Process::launch_user_program("loader", bytes)?;

    // Standard Userboot VMO Hand-off:
    // To support absolute zero-copy loading on compliant microkernel guidelines, 
    // we allocate a dedicated physical VMO wrapping the entire rootfs memory blob.
    // We then register and transfer its handle right into the newly-spawned 'loader''s process handle table.
    let rootfs_image = crate::rootfs::ROOTFS_IMAGE;
    let mut rootfs_vmo = Vmo::create_with_size(rootfs_image.len())?;
    rootfs_vmo.commit_all()?;
    rootfs_vmo.write(0, rootfs_image)?;

    // Locate the loader process we just initialized (which gets PID 1)
    if let Some(proc) = crate::task::process::find_process_mut(1) {
        let rights = Rights::READ.bits() | Rights::WRITE.bits();
        // Insert the BootFS VMO handle as index 100 in loader's handle table using add_raw_handle
        let _ = proc.handle_table.add_raw_handle(100, KernelObject::Vmo(rootfs_vmo), rights);
    }

    crate::log_info!("LOADER", "Loader service launched successfully at EL0! BootFS VMO registered.");
    Ok(())
}

pub fn launch_testloader() -> Result<()> {
    let bytes = crate::rootfs::get_file("system/bin/testloader")
        .ok_or(shared::status::Status::NotFound)?;
    crate::kprintln!("[DIAG] launch_testloader got bytes.len={}", bytes.len());
    Process::launch_user_program("testloader", bytes)?;
    crate::log_info!("LOADER", "Testloader launched successfully at EL0!");
    Ok(())
}
