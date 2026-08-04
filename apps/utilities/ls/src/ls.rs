use crate::env::{Dirent, FileSystem, FsError};
use crate::options::LsOptions;

/// 格式化数字到 byte 数组中而无需任何堆内存分配 (no_std 兼容)
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

/// 核心的流式读取和输出逻辑，绝不依赖任何动态内存分配（no_std 兼容）
pub fn run_ls<F: FileSystem>(env: &F, path: &str, opts: &LsOptions) -> Result<(), FsError> {
    let fd = env.open_dir(path)?;
    let mut dirent = Dirent::new();

    loop {
        match env.readdir(fd, &mut dirent) {
            Ok(false) => {
                // 读取到 EOF 正常退出
                break;
            }
            Ok(true) => {
                let name_len = dirent.name_len as usize;
                let name_bytes = &dirent.name[..name_len];

                // -a: 如果不带 -a 且文件名以 . 开头，则跳过
                if !opts.all && name_bytes.starts_with(b".") {
                    continue;
                }

                // 如果是 -l 长格式输出
                if opts.long {
                    // 1. 打印文件类型标志
                    if dirent.ftype == 2 {
                        env.write_stdout(b"d");
                    } else {
                        env.write_stdout(b"-");
                    }
                    env.write_stdout(b"rwxr-xr-x  1  root  root  "); // 简化模拟权限与所有者

                    // 2. 格式化并打印文件大小 (no_std 零内存分配)
                    let mut size_buf = [0u8; 20];
                    let size_len = format_u64(dirent.size, &mut size_buf);
                    // 右对齐对齐填充
                    let padding = 10 - size_len.min(10);
                    for _ in 0..padding {
                        env.write_stdout(b" ");
                    }
                    env.write_stdout(&size_buf[..size_len]);
                    env.write_stdout(b"  ");

                    // 3. 打印高亮文件名
                    if dirent.ftype == 2 && opts.classify {
                        env.write_stdout(b"\x1b[34m"); // 开启蓝色终端高亮
                        env.write_stdout(name_bytes);
                        env.write_stdout(b"/\x1b[0m\n"); // 关闭高亮，换行
                    } else {
                        env.write_stdout(name_bytes);
                        env.write_stdout(b"\n");
                    }
                } else {
                    // 非 -l 模式下：横向连续输出
                    if dirent.ftype == 2 && opts.classify {
                        env.write_stdout(b"\x1b[34m"); // 开启蓝色终端高亮
                        env.write_stdout(name_bytes);
                        env.write_stdout(b"/\x1b[0m  "); // 关闭高亮
                    } else {
                        env.write_stdout(name_bytes);
                        env.write_stdout(b"  ");
                    }
                }
            }
            Err(e) => {
                env.close_dir(fd);
                return Err(e);
            }
        }
    }

    if !opts.long {
        env.write_stdout(b"\n");
    }
    env.close_dir(fd);
    Ok(())
}
