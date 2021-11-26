use crate::capturable::Capturable;
use crate::protocol::{KeyboardEvent, PointerEvent, WheelEvent};

#[derive(PartialEq, Eq)]
pub enum InputDeviceType {
    AutoPilotDevice,
    UInputDevice,
    WindowsInput,
}

pub trait InputDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent);
    fn send_pointer_event(&mut self, event: &PointerEvent);
    fn send_keyboard_event(&mut self, event: &KeyboardEvent);
    fn set_capturable(&mut self, capturable: Box<dyn Capturable>);
    fn device_type(&self) -> InputDeviceType;
}
