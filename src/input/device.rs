use crate::protocol::{PointerEvent, KeyboardEvent};

pub trait InputDevice {
    fn send_pointer_event(&mut self, event: &PointerEvent);
    fn send_keyboard_event(&mut self, event: &KeyboardEvent);
}
