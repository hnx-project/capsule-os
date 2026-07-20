use crate::object::handle_table::HandleTable;
use crate::syscall::handlers;
use shared::status::Status;
use shared::types::HandleValue;

pub fn dispatch_capability(
    table: &HandleTable,
    syscall_num: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> Option<usize> {
    use shared::syscall_nums::*;

    let res = match syscall_num {
        SYSCALL_VMAR_MAP => {
            match handlers::memory::sys_vmar_map(
                table,
                arg0 as u32,
                arg1 as u32,
                arg2,
                arg3,
                arg4,
                arg5 as u32,
            ) {
                Ok(va) => va,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMAR_MAP_SELF => {
            match handlers::memory::sys_vmar_map_self(
                table,
                arg0 as u32,
                arg1,
                arg2,
                arg3 as u32,
            ) {
                Ok(va) => va,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMAR_UNMAP => match handlers::memory::sys_vmar_unmap(arg0, arg1) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        },

        SYSCALL_VMO_CREATE => {
            let size = arg0;
            match handlers::memory::sys_vmo_create(table, size) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_CREATE_CHILD => {
            let parent_handle = arg0 as u32;
            let offset = arg1;
            let size = arg2;
            // match handlers::memory::sys_vmo_create_child(table, parent_handle, offset, size) {
            //     Ok(hv) => hv.get() as usize,
            //     Err(e) => e.to_raw(),
            // }
            match handlers::memory::sys_vmo_create_child(table, parent_handle, offset, size) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_READ => {
            let handle = arg0 as u32;
            let offset = arg1;
            let dst_user_va = arg2;
            let len = arg3;
            match handlers::memory::sys_vmo_read(table, handle, offset, dst_user_va, len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_WRITE => {
            let handle = arg0 as u32;
            let offset = arg1;
            let src_user_va = arg2;
            let len = arg3;
            match handlers::memory::sys_vmo_write(table, handle, offset, src_user_va, len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_CREATE => match handlers::ipc::sys_channel_create(table) {
            Ok((h0, h1)) => (h0.get() as usize) | ((h1.get() as usize) << 32),
            Err(e) => e.to_raw(),
        },

        SYSCALL_CHANNEL_READ => {
            let handle = arg0 as u32;
            let buf_ptr = arg1;
            let buf_len = arg2;
            let handles_ptr = arg3;
            let handles_len = arg4;
            match handlers::ipc::sys_channel_read(table, handle, buf_ptr, buf_len, handles_ptr, handles_len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_WRITE => {
            let handle = arg0 as u32;
            let buf_ptr = arg1;
            let buf_len = arg2;
            let handles_ptr = arg3;
            let handles_len = arg4;
            match handlers::ipc::sys_channel_write(table, handle, buf_ptr, buf_len, handles_ptr, handles_len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_REGISTER => {
            let name_ptr = arg0;
            let name_len = arg1;
            let handle = arg2 as u32;
            match handlers::ipc::sys_channel_register(table, name_ptr, name_len, handle) {
                Ok(_) => 0,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_LOOKUP => {
            let name_ptr = arg0;
            let name_len = arg1;
            match handlers::ipc::sys_channel_lookup(table, name_ptr, name_len) {
                Ok(h) => h.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_HANDLE_DUPLICATE => {
            let handle = arg0 as u32;
            let rights = arg1 as u32;
            match table.duplicate_handle(HandleValue::new(handle), rights) {
                Ok(new_hv) => new_hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CLOSE => {
            let handle = arg0 as u32;
            match table.close(HandleValue::new(handle)) {
                Ok(_obj) => 0,
                Err(e) => e.to_raw(),
            }
        }

        _ => return None,
    };

    Some(res)
}
