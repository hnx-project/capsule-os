use crate::env::{FileSystem, FsError};
use crate::options::CatOptions;

/// 辅助：格式化数字至固定缓冲区 (no_std 友好，零堆分配)
fn format_u64(mut val: u64, buf: &mut [u8]) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut temp = [0u8; 20];
    let mut i = 0;
    while val > 0 {
        temp[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    let mut j = 0;
    while i > 0 {
        i -= 1;
        buf[j] = temp[i];
        j += 1;
    }
    j
}

/// 在控制台输出带对齐宽度的行号 (例如 "     1  ")
fn print_line_number<F: FileSystem>(env: &F, line_num: u64) {
    let mut num_buf = [0u8; 20];
    let len = format_u64(line_num, &mut num_buf);
    
    // 经典对齐：前置 6 位右对齐
    let padding = 6 - len.min(6);
    for _ in 0..padding {
        env.write_stdout(b" ");
    }
    env.write_stdout(&num_buf[..len]);
    env.write_stdout(b"  ");
}

/// 核心的流式读取和输出逻辑，支持 -n 高级参数，绝不依赖任何动态内存分配（no_std 兼容）
pub fn run_cat<F: FileSystem>(env: &F, path: &str, opts: &CatOptions) -> Result<(), FsError> {
    // 编译期静态分配 2KB 栈缓冲区，流式循环读取，不限文件大小，不占堆空间
    let mut buffer = [0u8; 2048];
    let fd = env.open(path)?;

    let mut line_num = 1u64;
    let mut start_of_line = true;

    loop {
        match env.read(fd, &mut buffer) {
            Ok(0) => {
                // 读取到 EOF 正常退出
                break;
            }
            Ok(bytes_read) => {
                let data = &buffer[..bytes_read];
                
                if !opts.number {
                    // 如果不需要显示行号，直接高效整块输出
                    env.write_stdout(data);
                } else {
                    // 流式分行扫描并按需打印行号
                    let mut start = 0;
                    for i in 0..bytes_read {
                        if start_of_line {
                            print_line_number(env, line_num);
                            line_num += 1;
                            start_of_line = false;
                        }

                        if data[i] == b'\n' {
                            env.write_stdout(&data[start..=i]);
                            start = i + 1;
                            start_of_line = true;
                        }
                    }
                    if start < bytes_read {
                        env.write_stdout(&data[start..]);
                    }
                }
            }
            Err(e) => {
                env.close(fd);
                return Err(e);
            }
        }
    }

    env.close(fd);
    Ok(())
}
