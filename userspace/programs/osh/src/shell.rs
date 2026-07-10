use crate::builtins::execute_builtin;
use crate::env::Environment;
use crate::parser::parse_line;

/// 执行单行命令，合并内建与外部执行的逻辑
pub fn execute_single_line<E: Environment>(env: &E, line: &str) {
    let trimmed_line = line.trim_end_matches(|c| c == '\r' || c == '\n');
    if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
        return;
    }

    if let Some(cmd) = parse_line(trimmed_line) {
        if !execute_builtin(env, &cmd) {
            let mut run_args = [""; 16];
            for i in 0..cmd.arg_count {
                run_args[i] = cmd.args[i];
            }
            match env.execute(cmd.name, &run_args[..cmd.arg_count]) {
                Ok(_) => {}
                Err(_) => {
                    env.write_stderr(b"osh: command not found: ");
                    env.write_stderr(cmd.name.as_bytes());
                    env.write_stderr(b"\n");
                }
            }
        }
    }
}

/// 支持执行外部脚本文件的能力 (Zsh 级批处理脚本)
pub fn run_script<E: Environment>(env: &E, path: &str) {
    let mut script_buf = [0u8; 4096];
    match env.read_file(path, &mut script_buf) {
        Ok(bytes_read) => {
            let script_str = match core::str::from_utf8(&script_buf[..bytes_read]) {
                Ok(s) => s,
                Err(_) => {
                    env.write_stderr(b"Error: Script file contains invalid UTF-8\n");
                    return;
                }
            };

            // 按行拆分执行
            let mut start = 0;
            let bytes = script_str.as_bytes();
            for i in 0..bytes.len() {
                if bytes[i] == b'\n' {
                    let line = &script_str[start..i];
                    execute_single_line(env, line);
                    start = i + 1;
                }
            }
            if start < bytes.len() {
                let line = &script_str[start..];
                execute_single_line(env, line);
            }
        }
        Err(_) => {
            env.write_stderr(b"Error: Failed to read script file: ");
            env.write_stderr(path.as_bytes());
            env.write_stderr(b"\n");
        }
    }
}

pub fn run_shell<E: Environment>(env: &E) -> ! {
    env.write_stdout(b"Welcome to osh!\nType 'help' for built-in commands.\n");

    let mut input_buf = [0u8; 1024];
    let mut cwd_buf = [0u8; 512];

    loop {
        // 渲染 Prompt: "osh:[path]$ "
        env.write_stdout(b"osh:");
        match env.getcwd(&mut cwd_buf) {
            Ok(len) => {
                env.write_stdout(&cwd_buf[..len]);
            }
            Err(_) => {
                env.write_stdout(b"?");
            }
        }
        env.write_stdout(b"$ ");

        // 读取一行输入
        match env.read_line(&mut input_buf) {
            Ok(bytes_read) => {
                if bytes_read == 0 {
                    continue;
                }

                let line_bytes = &input_buf[..bytes_read];
                let line_str = match core::str::from_utf8(line_bytes) {
                    Ok(s) => s,
                    Err(_) => {
                        env.write_stderr(b"Error: Invalid UTF-8 input\n");
                        continue;
                    }
                };

                execute_single_line(env, line_str);
            }
            Err(_) => {
                env.write_stderr(b"Error: Failed to read input\n");
            }
        }
    }
}
