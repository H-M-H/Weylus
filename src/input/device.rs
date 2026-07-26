use crate::capturable::Capturable;
use crate::protocol::{KeyboardEvent, PointerEvent, RelativePointerEvent, WheelEvent};

#[derive(PartialEq, Eq)]
pub enum InputDeviceType {
    AutoPilotDevice,
    UInputDevice,
    #[cfg(target_os = "windows")]
    WindowsInput,
}

pub trait InputDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent);
    fn send_touchpad_wheel_event(&mut self, event: &WheelEvent);
    fn send_pointer_event(&mut self, event: &PointerEvent);
    fn send_relative_pointer_event(&mut self, event: &RelativePointerEvent);
    fn release_buttons(&mut self);
    fn send_keyboard_event(&mut self, event: &KeyboardEvent);
    fn set_capturable(&mut self, capturable: Box<dyn Capturable>);
    fn device_type(&self) -> InputDeviceType;
}
