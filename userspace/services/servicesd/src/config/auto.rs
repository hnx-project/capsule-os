pub const MAX_SERVICES: usize = 16;
pub const MAX_DEPS: usize = 8;

pub struct ActiveService<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub dependencies: [&'a str; MAX_DEPS],
    pub dep_count: usize,
    pub is_program: bool,
}

pub static mut CONFIG_BUFFERS: [[u8; 512]; MAX_SERVICES] = [[0u8; 512]; MAX_SERVICES];

pub fn parse_auto_toml<'a>(content: &'a str) -> Option<ActiveService<'a>> {
    let mut name = "";
    let mut path = "";
    let mut dependencies = [""; MAX_DEPS];
    let mut dep_count = 0;
    let mut is_program = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }

        if let Some(eq_idx) = line.find('=') {
            let key = line[..eq_idx].trim();
            let mut val = line[eq_idx + 1..].trim();

            match key {
                "name" => {
                    name = val.trim_matches('"');
                }
                "path" => {
                    path = val.trim_matches('"');
                }
                "is_program" => {
                    is_program = val == "true";
                }
                "dependencies" => {
                    val = val.trim_start_matches('[').trim_end_matches(']');
                    for item in val.split(',') {
                        let item = item.trim().trim_matches('"');
                        if !item.is_empty() && dep_count < MAX_DEPS {
                            dependencies[dep_count] = item;
                            dep_count += 1;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if !name.is_empty() && !path.is_empty() {
        Some(ActiveService {
            name,
            path,
            dependencies,
            dep_count,
            is_program,
        })
    } else {
        None
    }
}
