use crate::protocol::PointerEvent;

pub trait PointerDevice {
    fn send_event(&mut self, event: &PointerEvent);
}
