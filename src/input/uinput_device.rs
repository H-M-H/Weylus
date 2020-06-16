use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use crate::input::device::InputDevice;
use crate::protocol::Button;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;
use crate::protocol::PointerType;
use crate::x11helper::Capturable;

use crate::cerror::CError;

use tracing::{trace, warn};

extern "C" {
    fn init_uinput_stylus(name: *const c_char, err: *mut CError) -> c_int;
    fn init_uinput_mouse(name: *const c_char, err: *mut CError) -> c_int;
    fn init_uinput_touch(name: *const c_char, err: *mut CError) -> c_int;
    fn destroy_uinput_device(fd: c_int);
    fn send_uinput_event(device: c_int, typ: c_int, code: c_int, value: c_int, err: *mut CError);
}

struct MultiTouch {
    id: i64,
}

pub struct GraphicTablet {
    stylus_fd: c_int,
    mouse_fd: c_int,
    touch_fd: c_int,
    touches: [Option<MultiTouch>; 5],
    capture: Capturable,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl GraphicTablet {
    pub fn new(capture: Capturable, id: String) -> Result<Self, CError> {
        let mut err = CError::new();
        let name_stylus = format!("Weylus Stylus - {}", id);
        let name_stylus_c_str = CString::new(name_stylus.as_bytes()).unwrap();
        let stylus_fd = unsafe { init_uinput_stylus(name_stylus_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            return Err(err);
        }
        let name_mouse = format!("Weylus Mouse - {}", id);
        let name_mouse_c_str = CString::new(name_mouse.as_bytes()).unwrap();

        let mouse_fd = unsafe { init_uinput_mouse(name_mouse_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            unsafe { destroy_uinput_device(stylus_fd) };
            return Err(err);
        }
        let name_touch = format!("Weylus Touch - {}", id);
        let name_touch_c_str = CString::new(name_touch.as_bytes()).unwrap();
        let touch_fd = unsafe { init_uinput_touch(name_touch_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            unsafe { destroy_uinput_device(stylus_fd) };
            unsafe { destroy_uinput_device(mouse_fd) };
            return Err(err);
        }
        std::thread::spawn(move || {
            if let Some(mut x11ctx) = crate::x11helper::X11Context::new() {
                // give X some time to register the new devices
                // and wait long enough to override whatever your desktop
                // environment decides to do once it detects them
                std::thread::sleep(std::time::Duration::from_secs(3));

                // map them to the whole screen and not only one monitor
                let res1 = x11ctx.map_input_device_to_entire_screen(&name_mouse);
                let res2 = x11ctx.map_input_device_to_entire_screen(&name_touch);
                // for some reason the stylus does not support a Coordinate Transformation Matrix
                // probably because no touch events are registered and thus the mapping is already
                // correct
                if res1.is_ok() && res2.is_ok() {
                    trace!("Succeeded mapping input devices to screen!");
                    return;
                }
                warn!("Failed to map input devices to screen!");
            }
        });
        let tblt = Self {
            stylus_fd,
            mouse_fd,
            touch_fd,
            touches: Default::default(),
            capture,
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        Ok(tblt)
    }

    fn transform_x(&self, x: f64) -> i32 {
        let x = (x * self.width + self.x) * ABS_MAX;
        x as i32
    }

    fn transform_y(&self, y: f64) -> i32 {
        let y = (y * self.height + self.y) * ABS_MAX;
        y as i32
    }

    fn transform_pressure(&self, p: f64) -> i32 {
        (p * ABS_MAX) as i32
    }

    fn transform_touch_size(&self, s: f64) -> i32 {
        (s * ABS_MAX) as i32
    }

    fn find_slot(&self, id: i64) -> Option<usize> {
        self.touches
            .iter()
            .enumerate()
            .find_map(|(slot, mt)| match mt {
                Some(mt) => {
                    if mt.id == id {
                        Some(slot)
                    } else {
                        None
                    }
                }
                _ => None,
            })
    }

    fn send(&self, fd: c_int, typ: c_int, code: c_int, value: c_int) {
        let mut err = CError::new();
        unsafe {
            send_uinput_event(fd, typ, code, value, &mut err);
        }
        if err.is_err() {
            warn!("{}", err);
        }
    }
}

impl Drop for GraphicTablet {
    fn drop(&mut self) {
        unsafe {
            destroy_uinput_device(self.stylus_fd);
            destroy_uinput_device(self.mouse_fd);
            destroy_uinput_device(self.touch_fd);
        };
    }
}

// Event Types
const ET_SYNC: c_int = 0x00;
const ET_KEY: c_int = 0x01;
//const ET_RELATIVE: c_int = 0x02;
const ET_ABSOLUTE: c_int = 0x03;
const ET_MSC: c_int = 0x04;

// Event Codes
const EC_SYNC_REPORT: c_int = 0;

const EC_KEY_MOUSE_LEFT: c_int = 0x110;
const EC_KEY_MOUSE_RIGHT: c_int = 0x111;
const EC_KEY_MOUSE_MIDDLE: c_int = 0x112;
const EC_KEY_TOOL_PEN: c_int = 0x140;
const EC_KEY_TOUCH: c_int = 0x14a;
const EC_KEY_TOOL_FINGER: c_int = 0x145;
const EC_KEY_TOOL_DOUBLETAP: c_int = 0x14d;
const EC_KEY_TOOL_TRIPLETAP: c_int = 0x14e;
const EC_KEY_TOOL_QUADTAP: c_int = 0x14f; /* Four fingers on trackpad */
const EC_KEY_TOOL_QUINTTAP: c_int = 0x148; /* Five fingers on trackpad */
//const EC_RELATIVE_X: c_int = 0x00;
//const EC_RELATIVE_Y: c_int = 0x01;

const EC_ABSOLUTE_X: c_int = 0x00;
const EC_ABSOLUTE_Y: c_int = 0x01;
const EC_ABSOLUTE_PRESSURE: c_int = 0x18;
const EC_ABSOLUTE_TILT_X: c_int = 0x1a;
const EC_ABSOLUTE_TILT_Y: c_int = 0x1b;
const EC_ABS_MT_SLOT: c_int = 0x2f; /* MT slot being modified */
const EC_ABS_MT_TOUCH_MAJOR: c_int = 0x30; /* Major axis of touching ellipse */
const EC_ABS_MT_TOUCH_MINOR: c_int = 0x31; /* Minor axis (omit if circular) */
const EC_ABS_MT_ORIENTATION: c_int = 0x34; /* Ellipse orientation */
const EC_ABS_MT_POSITION_X: c_int = 0x35; /* Center X touch position */
const EC_ABS_MT_POSITION_Y: c_int = 0x36; /* Center Y touch position */
const EC_ABS_MT_TRACKING_ID: c_int = 0x39; /* Unique ID of initiated contact */
const EC_ABS_MT_PRESSURE: c_int = 0x3a; /* Pressure on contact area */

const EC_MSC_TIMESTAMP: c_int = 0x05;

// This is choosen somewhat arbitrarily
// describes maximum value for ABS_X, ABS_Y, ABS_...
// This corresponds to PointerEvent values of 1.0
const ABS_MAX: f64 = 65535.0;

impl InputDevice for GraphicTablet {
    fn send_event(&mut self, event: &PointerEvent) {
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
        self.x = geometry.x;
        self.y = geometry.y;
        self.width = geometry.width;
        self.height = geometry.height;
        match event.pointer_type {
            PointerType::Touch => {
                match event.event_type {
                    PointerEventType::DOWN | PointerEventType::MOVE => {
                        let slot: usize;
                        // check if this event is already assigned to one of our 10 multitouch slots
                        if let Some(s) = self.find_slot(event.pointer_id) {
                            slot = s;
                        } else {
                            // this event is not assigned to a slot, lets try to do so now
                            // find the first unused slot
                            if let Some(s) =
                                self.touches
                                    .iter()
                                    .enumerate()
                                    .find_map(|(slot, mt)| match mt {
                                        None => Some(slot),
                                        Some(_) => None,
                                    })
                            {
                                slot = s;
                                self.touches[slot] = Some(MultiTouch {
                                    id: event.pointer_id,
                                })
                            } else {
                                // out of slots, do nothing
                                return;
                            }
                        };
                        self.send(self.touch_fd, ET_ABSOLUTE, EC_ABS_MT_SLOT, slot as i32);
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABS_MT_TRACKING_ID,
                            slot as i32,
                        );

                        if let PointerEventType::DOWN = event.event_type {
                            self.send(self.touch_fd, ET_KEY, EC_KEY_TOUCH, 1);
                            match slot {
                                1 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_FINGER, 0),
                                2 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_DOUBLETAP, 0),
                                3 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_TRIPLETAP, 0),
                                4 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_QUADTAP, 0),
                                _ => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_QUINTTAP, 0),
                            }
                            match slot {
                                1 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_DOUBLETAP, 1),
                                2 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_TRIPLETAP, 1),
                                3 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_QUADTAP, 1),
                                4 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_QUINTTAP, 1),
                                _ => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_FINGER, 1),
                            }
                        }
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABS_MT_PRESSURE,
                            self.transform_pressure(event.pressure),
                        );
                        let major: i32;
                        let minor: i32;
                        let orientation = if event.height >= event.width {
                            major = self.transform_touch_size(event.height);
                            minor = self.transform_touch_size(event.width);
                            0
                        } else {
                            major = self.transform_touch_size(event.width);
                            minor = self.transform_touch_size(event.height);
                            1
                        };
                        self.send(self.touch_fd, ET_ABSOLUTE, EC_ABS_MT_TOUCH_MAJOR, major);
                        self.send(self.touch_fd, ET_ABSOLUTE, EC_ABS_MT_TOUCH_MINOR, minor);
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABS_MT_ORIENTATION,
                            orientation,
                        );
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABS_MT_POSITION_X,
                            self.transform_x(event.x),
                        );
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABS_MT_POSITION_Y,
                            self.transform_y(event.y),
                        );
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_X,
                            self.transform_x(event.x),
                        );
                        self.send(
                            self.touch_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_Y,
                            self.transform_y(event.y),
                        );
                        self.send(
                            self.touch_fd,
                            ET_MSC,
                            EC_MSC_TIMESTAMP,
                            event.timestamp as i32,
                        );
                        self.send(self.touch_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                    }
                    PointerEventType::CANCEL | PointerEventType::UP => {
                        // remove from slot
                        if let Some(slot) = self.find_slot(event.pointer_id) {
                            self.send(self.touch_fd, ET_ABSOLUTE, EC_ABS_MT_SLOT, slot as i32);
                            self.send(self.touch_fd, ET_ABSOLUTE, EC_ABS_MT_TRACKING_ID, -1);
                            self.send(self.touch_fd, ET_KEY, EC_KEY_TOUCH, 0);
                            self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_FINGER, 0);
                            match slot {
                                1 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_DOUBLETAP, 0),
                                2 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_TRIPLETAP, 0),
                                3 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_QUADTAP, 0),
                                4 => self.send(self.touch_fd, ET_KEY, EC_KEY_TOOL_QUINTTAP, 0),
                                _ => (),
                            }
                            self.send(
                                self.touch_fd,
                                ET_MSC,
                                EC_MSC_TIMESTAMP,
                                event.timestamp as i32,
                            );
                            self.send(self.touch_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                            self.touches[slot] = None;
                        }
                    }
                };
            }
            PointerType::Pen => {
                match event.event_type {
                    PointerEventType::DOWN | PointerEventType::MOVE => {
                        if let PointerEventType::DOWN = event.event_type {
                            self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_PEN, 1);
                        }
                        self.send(
                            self.stylus_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_X,
                            self.transform_x(event.x),
                        );
                        self.send(
                            self.stylus_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_Y,
                            self.transform_y(event.y),
                        );
                        self.send(
                            self.stylus_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_PRESSURE,
                            self.transform_pressure(event.pressure),
                        );
                        self.send(
                            self.stylus_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_TILT_X,
                            event.tilt_x,
                        );
                        self.send(
                            self.stylus_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_TILT_Y,
                            event.tilt_y,
                        );
                    }
                    PointerEventType::UP | PointerEventType::CANCEL => {
                        self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_PEN, 0);
                    }
                }
                self.send(
                    self.stylus_fd,
                    ET_MSC,
                    EC_MSC_TIMESTAMP,
                    event.timestamp as i32,
                );
                self.send(self.stylus_fd, ET_SYNC, EC_SYNC_REPORT, 0);
            }
            PointerType::Mouse | PointerType::Unknown => {
                match event.event_type {
                    PointerEventType::DOWN | PointerEventType::MOVE => {
                        if let PointerEventType::DOWN = event.event_type {
                            match event.button {
                                Button::PRIMARY => {
                                    self.send(self.mouse_fd, ET_KEY, EC_KEY_MOUSE_LEFT, 1)
                                }
                                Button::SECONDARY => {
                                    self.send(self.mouse_fd, ET_KEY, EC_KEY_MOUSE_RIGHT, 1)
                                }
                                Button::AUXILARY => {
                                    self.send(self.mouse_fd, ET_KEY, EC_KEY_MOUSE_MIDDLE, 1)
                                }
                                _ => (),
                            }
                        }
                        self.send(
                            self.mouse_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_X,
                            self.transform_x(event.x),
                        );
                        self.send(
                            self.mouse_fd,
                            ET_ABSOLUTE,
                            EC_ABSOLUTE_Y,
                            self.transform_y(event.y),
                        );
                    }
                    PointerEventType::UP | PointerEventType::CANCEL => match event.button {
                        Button::PRIMARY => self.send(self.mouse_fd, ET_KEY, EC_KEY_MOUSE_LEFT, 0),
                        Button::SECONDARY => {
                            self.send(self.mouse_fd, ET_KEY, EC_KEY_MOUSE_RIGHT, 0)
                        }
                        Button::AUXILARY => {
                            self.send(self.mouse_fd, ET_KEY, EC_KEY_MOUSE_MIDDLE, 0)
                        }
                        _ => (),
                    },
                }
                self.send(
                    self.mouse_fd,
                    ET_MSC,
                    EC_MSC_TIMESTAMP,
                    event.timestamp as i32,
                );
                self.send(self.mouse_fd, ET_SYNC, EC_SYNC_REPORT, 0);
            }
        }
    }
}
