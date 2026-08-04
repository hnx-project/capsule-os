use crate::env::{Environment, ShellError};
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, cmd: &Command) {
    let target_path = if cmd.arg_count > 0 {
        cmd.args[0]
    } else {
        env.write_stderr(b"Error: cd requires a path argument\n");
        return;
    };

    match env.chdir(target_path) {
        Ok(_) => {}
        Err(ShellError::PathNotFound) => {
            env.write_stderr(b"Error: Path not found: ");
            env.write_stderr(target_path.as_bytes());
            env.write_stderr(b"\n");
        }
        Err(ShellError::PermissionDenied) => {
            env.write_stderr(b"Error: Permission denied\n");
        }
        Err(_) => {
            env.write_stderr(b"Error: Failed to change directory\n");
        }
    }
}
