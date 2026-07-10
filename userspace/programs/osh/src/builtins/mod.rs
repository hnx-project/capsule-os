pub mod cd;
pub mod echo;
pub mod env_cmd;
pub mod exit;
pub mod export;
pub mod help;
pub mod pwd;
pub mod type_cmd;

use crate::env::Environment;
use crate::parser::Command;

/// 尝试路由并执行内置命令。如果是内置命令，则返回 true，否则返回 false。
pub fn execute_builtin<E: Environment>(env: &E, cmd: &Command) -> bool {
    match cmd.name {
        "cd" => {
            cd::run(env, cmd);
            true
        }
        "pwd" => {
            pwd::run(env, cmd);
            true
        }
        "echo" => {
            echo::run(env, cmd);
            true
        }
        "export" => {
            export::run(env, cmd);
            true
        }
        "env" => {
            env_cmd::run(env, cmd);
            true
        }
        "type" => {
            type_cmd::run(env, cmd);
            true
        }
        "help" => {
            help::run(env, cmd);
            true
        }
        "exit" => {
            exit::run(env, cmd);
            true
        }
        _ => false,
    }
}
