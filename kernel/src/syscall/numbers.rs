pub const SYSCALL_EXIT: u32 = 0;
pub const SYSCALL_WRITE: u32 = 1;
pub const SYSCALL_GET_TID: u32 = 2;
pub const SYSCALL_GET_PID: u32 = 3;

pub const SYSCALL_CHANNEL_CREATE: u32 = 10;
pub const SYSCALL_CHANNEL_READ: u32 = 11;
pub const SYSCALL_CHANNEL_WRITE: u32 = 12;
pub const SYSCALL_CHANNEL_CALL: u32 = 13;
pub const SYSCALL_CHANNEL_REGISTER: u32 = 14;
pub const SYSCALL_CHANNEL_LOOKUP: u32 = 15;
pub const SYSCALL_HANDLE_DUPLICATE: u32 = 16;
pub const SYSCALL_PORT_CREATE: u32 = 20;
pub const SYSCALL_PORT_WAIT: u32 = 21;
pub const SYSCALL_PORT_QUEUE: u32 = 22;

pub const SYSCALL_VMO_CREATE: u32 = 30;
pub const SYSCALL_VMO_READ: u32 = 31;
pub const SYSCALL_VMO_WRITE: u32 = 32;
pub const SYSCALL_VMO_GET_SIZE: u32 = 33;
pub const SYSCALL_VMO_SET_SIZE: u32 = 34;

pub const SYSCALL_VMAR_MAP: u32 = 40;
pub const SYSCALL_VMAR_UNMAP: u32 = 41;
pub const SYSCALL_VMAR_PROTECT: u32 = 42;

pub const SYSCALL_THREAD_CREATE: u32 = 50;
pub const SYSCALL_THREAD_START: u32 = 51;
pub const SYSCALL_THREAD_EXIT: u32 = 52;

pub const SYSCALL_PROCESS_CREATE: u32 = 60;
pub const SYSCALL_PROCESS_START: u32 = 61;
pub const SYSCALL_PROCESS_EXIT: u32 = 62;

pub const SYSCALL_EVENT_CREATE: u32 = 70;
pub const SYSCALL_EVENT_SIGNAL: u32 = 71;
pub const SYSCALL_EVENT_ACK: u32 = 72;

pub const SYSCALL_TIMER_CREATE: u32 = 80;
pub const SYSCALL_TIMER_SET: u32 = 81;
pub const SYSCALL_TIMER_CANCEL: u32 = 82;

pub const SYSCALL_FUTEX_WAIT: u32 = 90;
pub const SYSCALL_FUTEX_WAKE: u32 = 91;

pub const SYSCALL_OPEN: u32 = 100;
pub const SYSCALL_CLOSE: u32 = 101;
pub const SYSCALL_READ: u32 = 102;
pub const SYSCALL_SEEK: u32 = 103;
pub const SYSCALL_GETCWD: u32 = 104;
pub const SYSCALL_CHDIR: u32 = 105;

pub const SYSCALL_EXEC: u32 = 110;
pub const SYSCALL_LOAD_BINARY: u32 = 111;
pub const SYSCALL_EXECVE: u32 = 112;

pub const SYSCALL_NR: u32 = 113;
