pub struct Command {
    _program: crate::string::String,
}

impl Command {
    pub fn new(program: &str) -> Self {
        let mut p = crate::string::String::new();
        let _ = p.push_str(program);
        Self { _program: p }
    }

    pub fn spawn(&mut self) -> Result<(), ()> {
        panic!("CapsuleOS: std::process::Command::spawn() is not implemented yet");
    }
}
