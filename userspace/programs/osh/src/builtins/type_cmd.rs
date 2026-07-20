use crate::env::Environment;
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, cmd: &Command) {
    if cmd.arg_count == 0 {
        env.write_stderr(b"Error: type requires an argument\n");
        return;
    }
    let target = cmd.args[0];
    match target {
        "cd" | "pwd" | "exit" | "help" | "echo" | "export" | "env" | "type" => {
            env.write_stdout(target.as_bytes());
            env.write_stdout(b" is a shell builtin\n");
        }
        _ => {
            env.write_stdout(target.as_bytes());
            env.write_stdout(b" not found as a builtin\n");
        }
    }
}
