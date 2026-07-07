use shared::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntNumber {
    Irq(u32),
    Fiq(u32),
    SError(u32),
    Synchronous,
    Invalid,
}

pub trait InterruptHandler: Send + Sync {
    fn handle(&self, int_num: IntNumber) -> Status;
}

pub trait InterruptController: Send + Sync {
    fn enable(&self, irq: u32) -> Status;
    fn disable(&self, irq: u32) -> Status;
    fn register_handler(&self, irq: u32, handler: &'static dyn InterruptHandler) -> Status;
    fn wait_for_interrupt() -> IntNumber;
    fn ack_interrupt(int_num: IntNumber);
}
