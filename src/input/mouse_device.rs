use autopilot::mouse;
use autopilot::screen::size as screen_size;

use tracing::warn;

use crate::input::device::InputDevice;
use crate::protocol::Button;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;
use crate::protocol::PointerType;

#[cfg(target_os = "linux")]
use crate::x11helper::Capturable;

#[cfg(target_os = "linux")]
pub struct Mouse {
    capture: Capturable,
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
}

#[cfg(not(target_os = "linux"))]
pub struct Mouse {
    enable_mouse: bool,
    enable_stylus: bool,
    enable_touch: bool,
}

#[cfg(target_os = "linux")]
impl Mouse {
    pub fn new(
        capture: Capturable,
        enable_mouse: bool,
        enable_stylus: bool,
        enable_touch: bool,
    ) -> Self {
        Self {
            capture,
            enable_mouse,
            enable_stylus,
            enable_touch,
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Mouse {
    pub fn new(enable_mouse: bool, enable_stylus: bool, enable_touch: bool) -> Self {
        Self {
            enable_mouse,
            enable_stylus,
            enable_touch,
        }
    }
}

impl InputDevice for Mouse {
    fn send_event(&mut self, event: &PointerEvent) {
        match event.pointer_type {
            PointerType::Mouse | PointerType::Unknown => {
                if !self.enable_mouse {
                    return;
                }
            }
            PointerType::Pen => {
                if !self.enable_stylus {
                    return;
                }
            }
            PointerType::Touch => {
                if !self.enable_touch {
                    return;
                }
            }
        }
        if !event.is_primary {
            return;
        }
        #[cfg(target_os = "linux")]
        {
            if let Err(err) = self.capture.before_input() {
                warn!("Failed to activate window, sending no input ({})", err);
                return;
            }
            let geometry = self.capture.geometry();
            if let Err(err) = geometry {
                warn!("Failed to get window geometry, sending no input ({})", err);
                return;
            }
            let geometry = geometry.unwrap();
            if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
                (event.x * geometry.width + geometry.x) * screen_size().width,
                (event.y * geometry.height + geometry.y) * screen_size().height,
            )) {
                warn!("Could not move mouse: {}", err);
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
                event.x * screen_size().width,
                event.y * screen_size().height,
            )) {
                warn!("Could not move mouse: {}", err);
            }
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
}
