use shared::status::Result;
use crate::task::Process;

pub fn launch_loader() -> Result<()> {
    let bytes = include_bytes!("../files/loader");
    Process::launch_user_program("loader", bytes)?;
    crate::log_info!("LOADER", "Loader service launched successfully at EL0!");
    Ok(())
}
