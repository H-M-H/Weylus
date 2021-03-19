use crate::protocol::{WheelEvent, PointerEvent, KeyboardEvent};

pub trait InputDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent);
    fn send_pointer_event(&mut self, event: &PointerEvent);
    fn send_keyboard_event(&mut self, event: &KeyboardEvent);
}
