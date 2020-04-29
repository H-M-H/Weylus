use autopilot::mouse;
use autopilot::screen::size as screen_size;

use tracing::warn;

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
    fn send_event(&mut self, event: &PointerEvent) {
        if !event.is_primary {
            return;
        }
        if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
            event.x * screen_size().width,
            event.y * screen_size().height,
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
