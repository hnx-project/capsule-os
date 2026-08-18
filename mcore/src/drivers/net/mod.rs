use shared::status::Result;

pub trait NetDriver: Send + Sync {
    fn send_packet(&self, buf: &[u8]) -> Result<()>;
    fn recv_packet(&self, buf: &mut [u8]) -> Result<usize>;
}

pub static ACTIVE_NET_DEVICE: spin::Mutex<Option<&'static dyn NetDriver>> = spin::Mutex::new(None);
