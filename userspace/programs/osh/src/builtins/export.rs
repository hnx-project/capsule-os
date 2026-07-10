use crate::env::Environment;
use crate::parser::Command;

pub fn run<E: Environment>(env: &E, cmd: &Command) {
    if cmd.arg_count == 0 {
        env.write_stderr(b"Error: export requires an argument like KEY=VALUE\n");
        return;
    }
    let arg = cmd.args[0];
    if let Some(pos) = arg.find('=') {
        let key = &arg[..pos];
        let val = &arg[pos + 1..];
        if key.is_empty() {
            env.write_stderr(b"Error: Invalid export key\n");
        } else {
            let _ = env.set_env(key, val);
        }
    } else {
        env.write_stderr(b"Error: Invalid export format, use KEY=VALUE\n");
    }
}
