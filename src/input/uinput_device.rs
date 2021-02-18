use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use crate::input::device::InputDevice;
use crate::protocol::Button;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;
use crate::protocol::PointerType;
use crate::x11helper::{Capturable, X11Context};

use crate::cerror::CError;

use tracing::warn;

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
    name_mouse_device: String,
    name_stylus_device: String,
    name_touch_device: String,
    num_mouse_mapping_tries: usize,
    num_stylus_mapping_tries: usize,
    num_touch_mapping_tries: usize,
    x11ctx: Option<X11Context>,
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
            name_mouse_device: name_mouse,
            name_touch_device: name_touch,
            name_stylus_device: name_stylus,
            num_mouse_mapping_tries: 0,
            num_stylus_mapping_tries: 0,
            num_touch_mapping_tries: 0,
            x11ctx: X11Context::new(),
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

// This specifies how many times it should be attempted to map the input devices created via uinput
// to the entire screen and not only a single monitor. Actually this is a workaround because
// apparently it is impossible to set the correct mapping in a sane way. The reason is that X needs
// some time to register new input devices, which makes it impossible to configure them right after
// creation as the devices won't be available for configuration at that time. This means one has to
// wait an unspecified amount of time until the devices show up. But just sleeping for example 3
// seconds does not solve the issue either because the input device for the stylus does not show up
// if there has not been any input. As a matter of fact things are even more compilcated as for
// some reason the stylus device created via uinput creates two devices for X. One can not be
// mapped to the screen (this is the device that shows up with out the need to send actual inputs
// via uinput) and another one that can be mapped to the screen. But this is the device that
// requires sending inputs via uinput first other wise it does not show up. This is why this crude
// method of just setting the mapping forcefully on the first MAX_SCREEN_MAPPING_TRIES input events
// has been choosen. If anyone knows a better solution: PLEASE FIX THIS!
const MAX_SCREEN_MAPPING_TRIES: usize = 100;

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
                if self.num_touch_mapping_tries < MAX_SCREEN_MAPPING_TRIES {
                    if let Some(x11ctx) = &mut self.x11ctx {
                        x11ctx.map_input_device_to_entire_screen(&self.name_touch_device, false);
                    }
                    self.num_touch_mapping_tries += 1;
                }
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
                            (event.timestamp % (i32::MAX as u64 + 1)) as i32,
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
                                (event.timestamp % (i32::MAX as u64 + 1)) as i32,
                            );
                            self.send(self.touch_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                            self.touches[slot] = None;
                        }
                    }
                };
            }
            PointerType::Pen => {
                if self.num_stylus_mapping_tries < MAX_SCREEN_MAPPING_TRIES {
                    if let Some(x11ctx) = &mut self.x11ctx {
                        x11ctx.map_input_device_to_entire_screen(&self.name_stylus_device, true);
                    }
                }
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
                    (event.timestamp % (i32::MAX as u64 + 1)) as i32,
                );
                self.send(self.stylus_fd, ET_SYNC, EC_SYNC_REPORT, 0);
            }
            PointerType::Mouse | PointerType::Unknown => {
                if self.num_mouse_mapping_tries < MAX_SCREEN_MAPPING_TRIES {
                    if let Some(x11ctx) = &mut self.x11ctx {
                        x11ctx.map_input_device_to_entire_screen(&self.name_mouse_device, false);
                    }
                }
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
                    (event.timestamp % (i32::MAX as u64 + 1)) as i32,
                );
                self.send(self.mouse_fd, ET_SYNC, EC_SYNC_REPORT, 0);
            }
        }
    }
}
