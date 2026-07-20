use crate::env::Environment;
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, _cmd: &Command) {
    let mut buf = [0u8; 512];
    match env.getcwd(&mut buf) {
        Ok(len) => {
            env.write_stdout(&buf[..len]);
            env.write_stdout(b"\n");
        }
        Err(_) => {
            env.write_stderr(b"Error: Failed to get current directory\n");
        }
    }
}
