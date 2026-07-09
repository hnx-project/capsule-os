pub const RAMFS_MAX_FILES: usize = 64;
pub const RAMFS_MAX_NAME_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RamfsNodeType {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct RamfsNode {
    pub name: [u8; RAMFS_MAX_NAME_LEN],
    pub ntype: RamfsNodeType,
    pub size: usize,
    pub data: [u8; 4096],
    pub children: [Option<usize>; 16],
    pub child_count: usize,
}

impl RamfsNode {
    pub fn new_dir(name: &str) -> Option<Self> {
        let mut node = RamfsNode {
            name: [0u8; RAMFS_MAX_NAME_LEN],
            ntype: RamfsNodeType::Directory,
            size: 0,
            data: [0u8; 4096],
            children: [const { None }; 16],
            child_count: 0,
        };
        let name_bytes = name.as_bytes();
        if name_bytes.len() >= RAMFS_MAX_NAME_LEN {
            return None;
        }
        node.name[..name_bytes.len()].copy_from_slice(name_bytes);
        Some(node)
    }

    pub fn new_file(name: &str) -> Option<Self> {
        let mut node = RamfsNode {
            name: [0u8; RAMFS_MAX_NAME_LEN],
            ntype: RamfsNodeType::File,
            size: 0,
            data: [0u8; 4096],
            children: [const { None }; 16],
            child_count: 0,
        };
        let name_bytes = name.as_bytes();
        if name_bytes.len() >= RAMFS_MAX_NAME_LEN {
            return None;
        }
        node.name[..name_bytes.len()].copy_from_slice(name_bytes);
        Some(node)
    }

    pub fn add_child(&mut self, child_idx: usize) -> bool {
        if self.child_count >= 16 {
            return false;
        }
        self.children[self.child_count] = Some(child_idx);
        self.child_count += 1;
        true
    }

    pub fn find_child(&self, name: &str) -> Option<usize> {
        for i in 0..self.child_count {
            if let Some(idx) = self.children[i] {
                let child_name = unsafe { core::str::from_utf8_unchecked(&self.data[idx..]) };
                if child_name == name {
                    return Some(idx);
                }
            }
        }
        None
    }
}

pub struct RamFs {
    pub nodes: [Option<RamfsNode>; RAMFS_MAX_FILES],
    pub root_idx: Option<usize>,
    pub next_idx: usize,
}

impl RamFs {
    pub fn new() -> Self {
        let mut fs = RamFs {
            nodes: [const { None }; RAMFS_MAX_FILES],
            root_idx: None,
            next_idx: 1,
        };

        fs.root_idx = Some(0);
        fs.nodes[0] = RamfsNode::new_dir("/");
        fs
    }

    pub fn alloc_node(&mut self, node: RamfsNode) -> Option<usize> {
        for i in 0..RAMFS_MAX_FILES {
            if self.nodes[i].is_none() {
                self.nodes[i] = Some(node);
                return Some(i);
            }
        }
        None
    }

    pub fn get_node(&self, idx: usize) -> Option<&RamfsNode> {
        self.nodes[idx].as_ref()
    }

    pub fn get_node_mut(&mut self, idx: usize) -> Option<&mut RamfsNode> {
        self.nodes[idx].as_mut()
    }

    pub fn mkdir(&mut self, parent_idx: usize, name: &str) -> Option<usize> {
        // 先进行父目录类型校验，用一个独立的不可变借用块错开
        {
            let parent = self.nodes[parent_idx].as_ref()?;
            if parent.ntype != RamfsNodeType::Directory {
                return None;
            }
        }

        let new_node = RamfsNode::new_dir(name)?;
        let new_idx = self.alloc_node(new_node)?;

        // 分配新节点完后，获取 parent 并增加其子节点关系
        let parent = self.nodes[parent_idx].as_mut()?;
        parent.add_child(new_idx);
        Some(new_idx)
    }

    pub fn mkdir_path(&mut self, path: &str) -> Option<usize> {
        let clean = if path.starts_with('/') {
            &path[1..]
        } else {
            path
        };
        let mut current = self.root_idx?;
        if clean.is_empty() {
            return Some(current);
        }
        for component in clean.split('/').filter(|c| !c.is_empty()) {
            if let Some(existing) = self.find_child_idx(current, component) {
                current = existing;
                continue;
            }
            current = self.mkdir(current, component)?;
        }
        Some(current)
    }

    fn find_child_idx(&self, parent_idx: usize, name: &str) -> Option<usize> {
        let parent = self.nodes[parent_idx].as_ref()?;
        if parent.ntype != RamfsNodeType::Directory {
            return None;
        }
        for i in 0..parent.child_count {
            if let Some(child_idx) = parent.children[i] {
                if let Some(child) = self.nodes[child_idx].as_ref() {
                    let name_len = child
                        .name
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(RAMFS_MAX_NAME_LEN);
                    if let Ok(name_str) = core::str::from_utf8(&child.name[..name_len]) {
                        if name_str == name {
                            return Some(child_idx);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn parent_of(&self, child_idx: usize) -> Option<usize> {
        let root = self.root_idx?;
        if child_idx == root {
            return None;
        }
        for i in 0..RAMFS_MAX_FILES {
            if i == child_idx {
                continue;
            }
            if let Some(ref node) = self.nodes[i] {
                if node.ntype != RamfsNodeType::Directory {
                    continue;
                }
                for j in 0..node.child_count {
                    if node.children[j] == Some(child_idx) {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    pub fn create(&mut self, parent_idx: usize, name: &str) -> Option<usize> {
        // 先进行父目录类型校验
        {
            let parent = self.nodes[parent_idx].as_ref()?;
            if parent.ntype != RamfsNodeType::Directory {
                return None;
            }
        }

        let new_node = RamfsNode::new_file(name)?;
        let new_idx = self.alloc_node(new_node)?;

        let parent = self.nodes[parent_idx].as_mut()?;
        parent.add_child(new_idx);
        Some(new_idx)
    }

    pub fn write_file(&mut self, idx: usize, data: &[u8], offset: usize) -> usize {
        if let Some(ref mut node) = self.nodes[idx] {
            if node.ntype != RamfsNodeType::File {
                return 0;
            }

            let write_len = data.len().min(4096 - offset);
            node.data[offset..offset + write_len].copy_from_slice(&data[..write_len]);
            node.size = core::cmp::max(node.size, offset + write_len);
            write_len
        } else {
            0
        }
    }

    pub fn read_file(&self, idx: usize, buf: &mut [u8], offset: usize) -> usize {
        if let Some(ref node) = self.nodes[idx] {
            if node.ntype != RamfsNodeType::File {
                return 0;
            }

            let read_len = buf.len().min(node.size.saturating_sub(offset));
            buf[..read_len].copy_from_slice(&node.data[offset..offset + read_len]);
            read_len
        } else {
            0
        }
    }

    pub fn resolve_path(&self, path: &str) -> Option<usize> {
        let clean = if path.starts_with('/') {
            &path[1..]
        } else {
            path
        };
        if clean.is_empty() {
            return self.root_idx;
        }

        let mut current = self.root_idx?;
        for component in clean.split('/').filter(|c| !c.is_empty()) {
            let node = self.nodes[current].as_ref()?;
            if node.ntype != RamfsNodeType::Directory {
                return None;
            }
            let mut next = None;
            for i in 0..node.child_count {
                if let Some(child_idx) = node.children[i] {
                    if let Some(child) = self.nodes[child_idx].as_ref() {
                        let name_len = child
                            .name
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(RAMFS_MAX_NAME_LEN);
                        if let Ok(name_str) = core::str::from_utf8(&child.name[..name_len]) {
                            if name_str == component {
                                next = Some(child_idx);
                                break;
                            }
                        }
                    }
                }
            }
            current = next?;
        }
        Some(current)
    }

    pub fn rmdir(&mut self, parent_idx: usize, child_idx: usize) -> bool {
        let node = match self.nodes[child_idx].as_ref() {
            Some(n) => n,
            None => return false,
        };
        if node.ntype != RamfsNodeType::Directory {
            return false;
        }
        if node.child_count > 0 {
            return false;
        }
        if !self.detach_child(parent_idx, child_idx) {
            return false;
        }
        self.nodes[child_idx] = None;
        true
    }

    pub fn unlink(&mut self, parent_idx: usize, child_idx: usize) -> bool {
        let node = match self.nodes[child_idx].as_ref() {
            Some(n) => n,
            None => return false,
        };
        if node.ntype != RamfsNodeType::File {
            return false;
        }
        if !self.detach_child(parent_idx, child_idx) {
            return false;
        }
        self.nodes[child_idx] = None;
        true
    }

    fn detach_child(&mut self, parent_idx: usize, child_idx: usize) -> bool {
        let parent = match self.nodes[parent_idx].as_mut() {
            Some(p) => p,
            None => return false,
        };
        for i in 0..parent.child_count {
            if parent.children[i] == Some(child_idx) {
                parent.children[i] = None;
                for j in i..parent.child_count.saturating_sub(1) {
                    parent.children[j] = parent.children[j + 1];
                }
                parent.children[parent.child_count - 1] = None;
                parent.child_count -= 1;
                return true;
            }
        }
        false
    }
}
