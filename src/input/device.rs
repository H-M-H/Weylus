use crate::protocol::PointerEvent;

pub trait InputDevice {
    fn send_event(&mut self, event: &PointerEvent);
}
