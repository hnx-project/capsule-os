pub struct Command<'a> {
    pub name: &'a str,
    pub args: [&'a str; 16],
    pub arg_count: usize,
}

/// 解析单行输入，在不使用堆内存分配的前提下将命令行拆分成名称和参数列表。
/// 适用于 no_std 架构。
pub fn parse_line(line: &str) -> Option<Command<'_>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut name = "";
    let mut args = [""; 16];
    let mut arg_count = 0;
    let mut in_word = false;
    let mut start_idx = 0;

    let mut word_count = 0;
    let bytes = trimmed.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b' ' || byte == b'\t' || byte == b'\r' || byte == b'\n' {
            if in_word {
                let word = &trimmed[start_idx..i];
                if word_count == 0 {
                    name = word;
                } else if arg_count < 16 {
                    args[arg_count] = word;
                    arg_count += 1;
                }
                word_count += 1;
                in_word = false;
            }
        } else {
            if !in_word {
                start_idx = i;
                in_word = true;
            }
        }
    }

    // 处理最后一个单词（如果是以非空白符结尾）
    if in_word {
        let word = &trimmed[start_idx..];
        if word_count == 0 {
            name = word;
        } else if arg_count < 16 {
            args[arg_count] = word;
            arg_count += 1;
        }
    }

    if name.is_empty() {
        None
    } else {
        Some(Command {
            name,
            args,
            arg_count,
        })
    }
}
