use crate::env::Environment;
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, cmd: &Command) {
    for i in 0..cmd.arg_count {
        if i > 0 {
            env.write_stdout(b" ");
        }
        env.write_stdout(cmd.args[i].as_bytes());
    }
    env.write_stdout(b"\n");
}
