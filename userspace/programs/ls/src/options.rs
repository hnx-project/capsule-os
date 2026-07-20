#[derive(Debug, Clone, Copy)]
pub struct LsOptions {
    pub all: bool,      // -a: 显示隐藏文件
    pub long: bool,     // -l: 详细信息长格式列表显示
    pub classify: bool, // -F: 给目录加 / 标识，给可执行文件加 * 等 (我们默认支持目录渲染，但可以通过这个控制)
}

impl LsOptions {
    pub const fn new() -> Self {
        Self {
            all: false,
            long: false,
            classify: true,
        }
    }
}
