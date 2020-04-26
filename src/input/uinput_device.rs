use libc::{c_int, c_uchar};

use crate::input::pointer::PointerDevice;
use crate::protocol::Button;
use crate::protocol::ClientConfig;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;

use crate::cerror::CError;

use log::warn;

extern "C" {
    fn init_uinput(err: *mut CError) -> c_int;
    fn destroy_uinput(fd: c_int);
    fn send_uinput_event(device: c_int, typ: c_int, code: c_int, value: c_int, err: *mut CError);
}


pub struct GraphicTablet {
    fd: c_int,
    client_config: ClientConfig,
}

impl GraphicTablet {
    pub fn new() -> Result<Self, CError> {
        let mut err = CError::new();
        let fd = unsafe { init_uinput(&mut err) };
        if err.is_err() {
            return Err(err);
        }
        let tblt = Self {
            fd: fd,
            client_config: ClientConfig::default(),
        };
        Ok(tblt)
    }

    fn transform_x(&self, x: i64) -> i32 {
        let x = x as f64 * 65535.0 / self.client_config.width as f64;
        x as i32
    }

    fn transform_y(&self, y: i64) -> i32 {
        let y = y as f64 * 65535.0 / self.client_config.height as f64;
        y as i32
    }

    fn transform_pressure(&self, p: f64) -> i32 {
        (p * 65535.0) as i32
    }

    fn send_event_safe(&self, typ: c_int, code: c_int, value: c_int) {
        let mut err = CError::new();
        unsafe {
            send_uinput_event(self.fd, typ, code, value, &mut err);
        }
        if err.is_err() {
            warn!("{}", err);
        }
    }
}

impl Drop for GraphicTablet {
    fn drop(&mut self) {
        unsafe {
            destroy_uinput(self.fd);
        };
    }
}

// Event Types
const ET_SYNC: c_int = 0x00;
const ET_KEY: c_int = 0x01;
const ET_RELATIVE: c_int = 0x02;
const ET_ABSOLUTE: c_int = 0x03;

// Event Codes
const EC_SYNC_REPORT: c_int = 0x00;

const EC_KEY_TOOL_PEN: c_int = 0x140;
const EC_KEY_TOUCH: c_int = 0x14a;
const EC_KEY_STYLUS: c_int = 0x14b;

const EC_RELATIVE_X: c_int = 0x00;
const EC_RELATIVE_Y: c_int = 0x01;

const EC_ABSOLUTE_X: c_int = 0x00;
const EC_ABSOLUTE_Y: c_int = 0x01;
const EC_ABSOLUTE_PRESSURE: c_int = 0x18;
const EC_ABSOLUTE_TILT_X: c_int = 0x1a;
const EC_ABSOLUTE_TILT_Y: c_int = 0x1b;

// Maximum for Absolute Values
const ABS_MAX: c_int = 65535;

impl PointerDevice for GraphicTablet {
    fn send_event(&self, event: &PointerEvent) {
        self.send_event_safe(ET_ABSOLUTE, EC_ABSOLUTE_X, self.transform_x(event.screen_x));
        self.send_event_safe(ET_ABSOLUTE, EC_ABSOLUTE_Y, self.transform_y(event.screen_y));
        self.send_event_safe(
            ET_ABSOLUTE,
            EC_ABSOLUTE_PRESSURE,
            self.transform_pressure(event.pressure),
        );
        self.send_event_safe(ET_ABSOLUTE, EC_ABSOLUTE_TILT_X, event.tilt_x);
        self.send_event_safe(ET_ABSOLUTE, EC_ABSOLUTE_TILT_Y, event.tilt_y);
        match event.event_type {
            PointerEventType::DOWN => {
                self.send_event_safe(ET_KEY, EC_KEY_TOOL_PEN, 1);
            }
            PointerEventType::UP | PointerEventType::CANCEL => {
                self.send_event_safe(ET_KEY, EC_KEY_TOOL_PEN, 0);
            }
            PointerEventType::MOVE => ()
        }
        self.send_event_safe(ET_SYNC, EC_SYNC_REPORT, 1);
    }

    fn set_client_config(&mut self, config: ClientConfig) {
        self.client_config = config;
    }
}
