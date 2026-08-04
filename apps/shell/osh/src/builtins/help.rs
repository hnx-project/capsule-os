use crate::env::Environment;
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, _cmd: &Command) {
    env.write_stdout(b"osh - Capsule OS Shell\n");
    env.write_stdout(b"Built-in commands:\n");
    env.write_stdout(b"  cd <path>       Change current working directory\n");
    env.write_stdout(b"  pwd             Print current working directory\n");
    env.write_stdout(b"  echo [args]     Print messages to standard output\n");
    env.write_stdout(b"  export K=V      Set an environment variable\n");
    env.write_stdout(b"  env             Print all environment variables\n");
    env.write_stdout(b"  type <cmd>      Describe a command type\n");
    env.write_stdout(b"  exit            Exit the shell\n");
    env.write_stdout(b"  help            Show this help message\n");
}
