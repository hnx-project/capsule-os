use shared::status::Result;
use crate::task::Process;

pub fn launch_loader() -> Result<()> {
    let bytes = crate::rootfs::get_file("system/bin/loader")
        .ok_or(shared::status::Status::NotFound)?;
    Process::launch_user_program("loader", bytes)?;
    crate::log_info!("LOADER", "Loader service launched successfully at EL0!");
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
