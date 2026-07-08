use shared::status::{Result, Status};
use crate::vfs;

pub fn sys_open(path_ptr: usize, path_len: usize, flags: u32) -> Result<u32> {
    if path_ptr == 0 || path_len == 0 {
        return Err(Status::InvalidArgs);
    }
    let slice = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
    let path_str = core::str::from_utf8(slice).map_err(|_| Status::InvalidArgs)?;

    // Alloc a Vnode for this path (assume File)
    let vnode_id = vfs::alloc_vnode(vfs::VnodeType::File)?;

    // Allocate FD
    let fd = vfs::alloc_fd(vnode_id, flags)?;
    crate::log_info!("VFS", "sys_open: opened '{}' -> fd = {}, vnode = {}", path_str, fd, vnode_id);
    Ok(fd)
}

pub fn sys_close(fd: u32) -> Result<()> {
    if let Some(vnode_id) = vfs::get_fd(fd) {
        vfs::close_fd(fd);
        vfs::release_vnode(vnode_id);
        crate::log_info!("VFS", "sys_close: closed fd = {}, vnode = {}", fd, vnode_id);
        Ok(())
    } else {
        Err(Status::NotFound)
    }
}

pub fn sys_read(fd: u32, buf_ptr: usize, buf_len: usize) -> Result<usize> {
    if buf_ptr == 0 || buf_len == 0 {
        return Err(Status::InvalidArgs);
    }
    let _vnode_id = vfs::get_fd(fd).ok_or(Status::NotFound)?;
    let offset = vfs::read_fd_offset(fd).unwrap_or(0);

    // Copy a test pattern to userspace to mock successfully reading from VFS!
    let slice = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len) };
    let pattern = b"VFS_READ_OK";
    
    let read_len = core::cmp::min(buf_len, pattern.len().saturating_sub(offset as usize));
    if read_len > 0 {
        slice[..read_len].copy_from_slice(&pattern[offset as usize .. offset as usize + read_len]);
        vfs::write_fd_offset(fd, offset + read_len as u64);
        crate::log_info!("VFS", "sys_read: read {} bytes from fd = {}", read_len, fd);
        Ok(read_len)
    } else {
        Ok(0)
    }
}

pub fn sys_seek(fd: u32, offset: i64, whence: i32) -> Result<u64> {
    let _vnode_id = vfs::get_fd(fd).ok_or(Status::NotFound)?;
    let cur_offset = vfs::read_fd_offset(fd).unwrap_or(0) as i64;
    
    let new_offset = match whence {
        0 => offset,
        1 => cur_offset + offset,
        _ => return Err(Status::InvalidArgs),
    };
    
    if new_offset < 0 {
        return Err(Status::InvalidArgs);
    }
    
    vfs::write_fd_offset(fd, new_offset as u64);
    crate::log_info!("VFS", "sys_seek: seek fd = {} to new_offset = {}", fd, new_offset);
    Ok(new_offset as u64)
}
