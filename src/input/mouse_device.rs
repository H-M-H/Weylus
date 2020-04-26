use autopilot::mouse;

use log::warn;

use crate::input::pointer::PointerDevice;
use crate::protocol::Button;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;

pub struct Mouse {}

impl Mouse {
    pub fn new() -> Self {
        Mouse {}
    }
}

impl PointerDevice for Mouse {
    fn send_event(&self, event: &PointerEvent) {
        if !event.is_primary {
            return;
        }
        if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
            event.screen_x as f64,
            event.screen_y as f64,
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
            },
            _=>()
        }
    }
}
