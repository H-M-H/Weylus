use autopilot::mouse;
use autopilot::mouse::ScrollDirection;
use autopilot::screen::size as screen_size;

use tracing::warn;

use crate::input::device::{InputDevice, InputDeviceType};
use crate::protocol::{
    Button, KeyboardEvent, KeyboardEventType, PointerEvent, PointerEventType, WheelEvent,
};

use crate::screen_capture::Capturable;

pub struct AutoPilotDevice {
    capturable: Box<dyn Capturable>,
}

#[cfg(target_os = "linux")]
impl AutoPilotDevice {
    pub fn new(capturable: Box<dyn Capturable>) -> Self {
        Self { capturable }
    }
}

impl InputDevice for AutoPilotDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        match event.dy {
            1..=i32::MAX => mouse::scroll(ScrollDirection::Up, 1),
            i32::MIN..=-1 => mouse::scroll(ScrollDirection::Down, 1),
            0 => {}
        }
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        if !event.is_primary {
            return;
        }
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        let geometry = self.capturable.geometry_relative();
        if let Err(err) = geometry {
            warn!("Failed to get window geometry, sending no input ({})", err);
            return;
        }
        let (x_rel, y_rel, width_rel, height_rel) = geometry.unwrap();
        if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
            (event.x * width_rel + x_rel) * screen_size().width,
            (event.y * height_rel + y_rel) * screen_size().height,
        )) {
            warn!("Could not move mouse: {}", err);
        }
        match event.event_type {
            PointerEventType::DOWN => match event.button {
                Button::PRIMARY => mouse::toggle(mouse::Button::Left, true),
                Button::AUXILARY => mouse::toggle(mouse::Button::Middle, true),
                Button::SECONDARY => mouse::toggle(mouse::Button::Right, true),
                _ => (),
            },
            PointerEventType::UP => {
                mouse::toggle(mouse::Button::Left, false);
                mouse::toggle(mouse::Button::Middle, false);
                mouse::toggle(mouse::Button::Right, false);
            }
            _ => (),
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        use autopilot::key::{Character, Code, KeyCode};

        let state = match event.event_type {
            KeyboardEventType::UP => false,
            KeyboardEventType::DOWN => true,
            // autopilot doesn't handle this, so just do nothing
            KeyboardEventType::REPEAT => return,
        };

        fn map_key(code: &str) -> Option<KeyCode> {
            match code {
                "Escape" => Some(KeyCode::Escape),
                "Enter" => Some(KeyCode::Return),
                "Backspace" => Some(KeyCode::Backspace),
                "Tab" => Some(KeyCode::Tab),
                "Space" => Some(KeyCode::Space),
                "CapsLock" => Some(KeyCode::CapsLock),
                "F1" => Some(KeyCode::F1),
                "F2" => Some(KeyCode::F2),
                "F3" => Some(KeyCode::F3),
                "F4" => Some(KeyCode::F4),
                "F5" => Some(KeyCode::F5),
                "F6" => Some(KeyCode::F6),
                "F7" => Some(KeyCode::F7),
                "F8" => Some(KeyCode::F8),
                "F9" => Some(KeyCode::F9),
                "F10" => Some(KeyCode::F10),
                "F11" => Some(KeyCode::F11),
                "F12" => Some(KeyCode::F12),
                "F13" => Some(KeyCode::F13),
                "F14" => Some(KeyCode::F14),
                "F15" => Some(KeyCode::F15),
                "F16" => Some(KeyCode::F16),
                "F17" => Some(KeyCode::F17),
                "F18" => Some(KeyCode::F18),
                "F19" => Some(KeyCode::F19),
                "F20" => Some(KeyCode::F20),
                "F21" => Some(KeyCode::F21),
                "F22" => Some(KeyCode::F22),
                "F23" => Some(KeyCode::F23),
                "F24" => Some(KeyCode::F24),
                "Home" => Some(KeyCode::Home),
                "ArrowUp" => Some(KeyCode::UpArrow),
                "PageUp" => Some(KeyCode::PageUp),
                "ArrowLeft" => Some(KeyCode::LeftArrow),
                "ArrowRight" => Some(KeyCode::RightArrow),
                "End" => Some(KeyCode::End),
                "ArrowDown" => Some(KeyCode::DownArrow),
                "PageDown" => Some(KeyCode::PageDown),
                "Delete" => Some(KeyCode::Delete),
                "ControlLeft" | "ControlRight" => Some(KeyCode::Control),
                "AltLeft" | "AltRight" => Some(KeyCode::Alt),
                "MetaLeft" | "MetaRight" => Some(KeyCode::Meta),
                "ShiftLeft" | "ShiftRight" => Some(KeyCode::Shift),
                _ => None,
            }
        }
        let key = map_key(&event.code);
        let mut flags = Vec::new();
        if event.ctrl {
            flags.push(autopilot::key::Flag::Control);
        }
        if event.alt {
            flags.push(autopilot::key::Flag::Alt);
        }
        if event.meta {
            flags.push(autopilot::key::Flag::Meta);
        }
        if event.shift {
            flags.push(autopilot::key::Flag::Shift);
        }
        match key {
            Some(key) => autopilot::key::toggle(&Code(key), state, &flags, 0),
            None => {
                for c in event.key.chars() {
                    autopilot::key::toggle(&Character(c), state, &flags, 0);
                }
            }
        }
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::AutoPilotDevice
    }
}
