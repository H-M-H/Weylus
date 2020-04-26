use crate::protocol::{PointerEvent, ClientConfig};

pub trait PointerDevice {
    fn send_event(&self, event: &PointerEvent);

    fn set_client_config(&mut self, config: ClientConfig) {}
}
