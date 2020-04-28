use crate::protocol::PointerEvent;

pub trait PointerDevice {
    fn send_event(&self, event: &PointerEvent);
}
