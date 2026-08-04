pub struct CatOptions {
    pub number: bool, // -n: 显示行号
}

impl CatOptions {
    pub const fn new() -> Self {
        Self { number: false }
    }
}
