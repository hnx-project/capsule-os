use crate::env::Environment;
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, _cmd: &Command) {
    env.write_stdout(b"Goodbye!\n");
    env.exit(0);
}
