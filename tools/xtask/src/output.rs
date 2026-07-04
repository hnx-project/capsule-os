use std::process::{Command, Output};

pub struct SilentOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_silent<F>(cmd: &mut Command, on_success: F) -> SilentOutput
where
    F: FnOnce(),
{
    let output = cmd.output();
    match output {
        Ok(Output { status, stdout, stderr }) => {
            let stdout_str = String::from_utf8_lossy(&stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&stderr).to_string();
            if status.success() {
                on_success();
                SilentOutput { success: true, stdout: stdout_str, stderr: stderr_str }
            } else {
                eprintln!("{}", stdout_str);
                eprintln!("{}", stderr_str);
                SilentOutput { success: false, stdout: stdout_str, stderr: stderr_str }
            }
        }
        Err(e) => {
            eprintln!("Failed to execute command: {}", e);
            SilentOutput { success: false, stdout: String::new(), stderr: e.to_string() }
        }
    }
}
