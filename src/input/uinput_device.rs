use std::cmp::Ordering;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use crate::capturable::x11::X11Context;
use crate::capturable::{Capturable, Geometry};
use crate::input::device::{InputDevice, InputDeviceType};
use crate::protocol::{
    Button, KeyboardEvent, KeyboardEventType, KeyboardLocation, PointerEvent, PointerEventType,
    PointerType, WheelEvent,
};

use crate::cerror::CError;

use tracing::{debug, warn};

extern "C" {
    fn init_uinput_keyboard(name: *const c_char, err: *mut CError) -> c_int;
    fn init_uinput_stylus(name: *const c_char, err: *mut CError) -> c_int;
    fn init_uinput_mouse(name: *const c_char, err: *mut CError) -> c_int;
    fn init_uinput_touch(name: *const c_char, err: *mut CError) -> c_int;
    fn destroy_uinput_device(fd: c_int);
    fn send_uinput_event(device: c_int, typ: c_int, code: c_int, value: c_int, err: *mut CError);
}

struct MultiTouch {
    id: i64,
}

pub struct UInputDevice {
    keyboard_fd: c_int,
    stylus_fd: c_int,
    mouse_fd: c_int,
    touch_fd: c_int,
    touches: [Option<MultiTouch>; 5],
    tool_pen_active: bool,
    capturable: Box<dyn Capturable>,
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

impl UInputDevice {
    pub fn new(capturable: Box<dyn Capturable>, id: &Option<String>) -> Result<Self, CError> {
        let mut suffix = String::new();
        if let Some(id) = id {
            suffix = format!(" - {}", id);
        }
        let mut err = CError::new();
        let name_stylus = format!("Weylus Stylus{}", suffix);
        let name_stylus_c_str = CString::new(name_stylus.as_bytes()).unwrap();
        let stylus_fd = unsafe { init_uinput_stylus(name_stylus_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            return Err(err);
        }

        let name_mouse = format!("Weylus Mouse{}", suffix);
        let name_mouse_c_str = CString::new(name_mouse.as_bytes()).unwrap();
        let mouse_fd = unsafe { init_uinput_mouse(name_mouse_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            unsafe { destroy_uinput_device(stylus_fd) };
            return Err(err);
        }

        let name_touch = format!("Weylus Touch{}", suffix);
        let name_touch_c_str = CString::new(name_touch.as_bytes()).unwrap();
        let touch_fd = unsafe { init_uinput_touch(name_touch_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            unsafe { destroy_uinput_device(stylus_fd) };
            unsafe { destroy_uinput_device(mouse_fd) };
            return Err(err);
        }

        let name_keyboard = format!("Weylus Keyboard{}", suffix);
        let name_keyboard_c_str = CString::new(name_keyboard.as_bytes()).unwrap();
        let keyboard_fd = unsafe { init_uinput_keyboard(name_keyboard_c_str.as_ptr(), &mut err) };
        if err.is_err() {
            unsafe { destroy_uinput_device(stylus_fd) };
            unsafe { destroy_uinput_device(mouse_fd) };
            unsafe { destroy_uinput_device(touch_fd) };
            return Err(err);
        }

        let tblt = Self {
            keyboard_fd,
            stylus_fd,
            mouse_fd,
            touch_fd,
            touches: Default::default(),
            tool_pen_active: false,
            capturable,
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

impl Drop for UInputDevice {
    fn drop(&mut self) {
        unsafe {
            destroy_uinput_device(self.keyboard_fd);
            destroy_uinput_device(self.stylus_fd);
            destroy_uinput_device(self.mouse_fd);
            destroy_uinput_device(self.touch_fd);
        };
    }
}

// Event Types
const ET_SYNC: c_int = 0x00;
const ET_KEY: c_int = 0x01;
const ET_RELATIVE: c_int = 0x02;
const ET_ABSOLUTE: c_int = 0x03;
const ET_MSC: c_int = 0x04;

// Event Codes
const EC_SYNC_REPORT: c_int = 0;

const EC_KEY_MOUSE_LEFT: c_int = 0x110;
const EC_KEY_MOUSE_RIGHT: c_int = 0x111;
const EC_KEY_MOUSE_MIDDLE: c_int = 0x112;
const EC_KEY_TOOL_PEN: c_int = 0x140;
const EC_KEY_TOOL_RUBBER: c_int = 0x141;
const EC_KEY_TOUCH: c_int = 0x14a;
const EC_KEY_TOOL_FINGER: c_int = 0x145;
const EC_KEY_TOOL_DOUBLETAP: c_int = 0x14d;
const EC_KEY_TOOL_TRIPLETAP: c_int = 0x14e;
const EC_KEY_TOOL_QUADTAP: c_int = 0x14f; /* Four fingers on trackpad */
const EC_KEY_TOOL_QUINTTAP: c_int = 0x148; /* Five fingers on trackpad */
//const EC_RELATIVE_X: c_int = 0x00;
//const EC_RELATIVE_Y: c_int = 0x01;

const EC_REL_HWHEEL: c_int = 0x06;
const EC_REL_WHEEL: c_int = 0x08;
const EC_REL_WHEEL_HI_RES: c_int = 0x0b;
const EC_REL_HWHEEL_HI_RES: c_int = 0x0c;

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

impl InputDevice for UInputDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }

        fn direction(d: i32) -> i32 {
            match d.cmp(&0) {
                Ordering::Equal => 0,
                Ordering::Less => -1,
                Ordering::Greater => 1,
            }
        }

        self.send(
            self.mouse_fd,
            ET_RELATIVE,
            EC_REL_WHEEL,
            direction(event.dy),
        );
        self.send(
            self.mouse_fd,
            ET_RELATIVE,
            EC_REL_HWHEEL,
            direction(event.dy),
        );
        self.send(self.mouse_fd, ET_RELATIVE, EC_REL_WHEEL_HI_RES, event.dx);
        self.send(self.mouse_fd, ET_RELATIVE, EC_REL_HWHEEL_HI_RES, event.dx);

        self.send(
            self.mouse_fd,
            ET_MSC,
            EC_MSC_TIMESTAMP,
            (event.timestamp % (i32::MAX as u64 + 1)) as i32,
        );
        self.send(self.mouse_fd, ET_SYNC, EC_SYNC_REPORT, 0);
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        let geometry = self.capturable.geometry();
        if let Err(err) = geometry {
            warn!("Failed to get window geometry, sending no input ({})", err);
            return;
        }
        let (x, y, width, height) = match geometry.unwrap() {
            Geometry::Relative(x, y, width, height) => (x, y, width, height),
            _ => unreachable!(),
        };
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
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
                    self.num_touch_mapping_tries += 1;
                }
                match event.event_type {
                    PointerEventType::DOWN | PointerEventType::MOVE => {
                        if let PointerEventType::DOWN = event.event_type {
                            self.send(self.stylus_fd, ET_KEY, EC_KEY_TOUCH, 1);
                        }
                        if !self.tool_pen_active && !event.buttons.contains(Button::ERASER) {
                            self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_PEN, 1);
                            self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_RUBBER, 0);
                            self.tool_pen_active = true;
                        }
                        if let Button::ERASER = event.button {
                            self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_PEN, 0);
                            self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_RUBBER, 1);
                            self.tool_pen_active = false;
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
                        self.send(self.stylus_fd, ET_KEY, EC_KEY_TOUCH, 0);
                        self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_PEN, 0);
                        self.send(self.stylus_fd, ET_KEY, EC_KEY_TOOL_RUBBER, 0);
                        self.send(self.stylus_fd, ET_ABSOLUTE, EC_ABSOLUTE_PRESSURE, 0);
                        self.tool_pen_active = false;
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
                    self.num_touch_mapping_tries += 1;
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

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        use crate::input::uinput_keys::*;
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        fn map_key(code: &str, location: &KeyboardLocation) -> c_int {
            match (code, location) {
                ("Escape", _) => KEY_ESC,
                ("Digit0", KeyboardLocation::NUMPAD) => KEY_KP0,
                ("Digit1", KeyboardLocation::NUMPAD) => KEY_KP1,
                ("Digit2", KeyboardLocation::NUMPAD) => KEY_KP2,
                ("Digit3", KeyboardLocation::NUMPAD) => KEY_KP3,
                ("Digit4", KeyboardLocation::NUMPAD) => KEY_KP4,
                ("Digit5", KeyboardLocation::NUMPAD) => KEY_KP5,
                ("Digit6", KeyboardLocation::NUMPAD) => KEY_KP6,
                ("Digit7", KeyboardLocation::NUMPAD) => KEY_KP7,
                ("Digit8", KeyboardLocation::NUMPAD) => KEY_KP8,
                ("Digit9", KeyboardLocation::NUMPAD) => KEY_KP9,
                ("Minus", KeyboardLocation::NUMPAD) => KEY_KPMINUS,
                ("Equal", KeyboardLocation::NUMPAD) => KEY_KPEQUAL,
                ("Enter", KeyboardLocation::NUMPAD) => KEY_KPENTER,
                ("Digit0", _) => KEY_0,
                ("Digit1", _) => KEY_1,
                ("Digit2", _) => KEY_2,
                ("Digit3", _) => KEY_3,
                ("Digit4", _) => KEY_4,
                ("Digit5", _) => KEY_5,
                ("Digit6", _) => KEY_6,
                ("Digit7", _) => KEY_7,
                ("Digit8", _) => KEY_8,
                ("Digit9", _) => KEY_9,
                ("Minus", _) => KEY_MINUS,
                ("Equal", _) => KEY_EQUAL,
                ("Enter", _) => KEY_ENTER,
                ("Backspace", _) => KEY_BACKSPACE,
                ("Tab", _) => KEY_TAB,
                ("KeyA", _) => KEY_A,
                ("KeyB", _) => KEY_B,
                ("KeyC", _) => KEY_C,
                ("KeyD", _) => KEY_D,
                ("KeyE", _) => KEY_E,
                ("KeyF", _) => KEY_F,
                ("KeyG", _) => KEY_G,
                ("KeyH", _) => KEY_H,
                ("KeyI", _) => KEY_I,
                ("KeyJ", _) => KEY_J,
                ("KeyK", _) => KEY_K,
                ("KeyL", _) => KEY_L,
                ("KeyM", _) => KEY_M,
                ("KeyN", _) => KEY_N,
                ("KeyO", _) => KEY_O,
                ("KeyP", _) => KEY_P,
                ("KeyQ", _) => KEY_Q,
                ("KeyR", _) => KEY_R,
                ("KeyS", _) => KEY_S,
                ("KeyT", _) => KEY_T,
                ("KeyU", _) => KEY_U,
                ("KeyV", _) => KEY_V,
                ("KeyW", _) => KEY_W,
                ("KeyX", _) => KEY_X,
                ("KeyY", _) => KEY_Y,
                ("KeyZ", _) => KEY_Z,
                ("BracketLeft", _) => KEY_LEFTBRACE,
                ("BracketRight", _) => KEY_RIGHTBRACE,
                ("Semicolon", _) => KEY_SEMICOLON,
                ("Quote", _) => KEY_APOSTROPHE,
                ("Backquote", _) => KEY_GRAVE,
                ("Backslash", _) => KEY_BACKSLASH,
                ("Comma", _) => KEY_COMMA,
                ("Period", _) => KEY_DOT,
                ("Slash", _) => KEY_SLASH,
                ("Space", _) => KEY_SPACE,
                ("CapsLock", _) => KEY_CAPSLOCK,
                ("NumpadMultiply", _) => KEY_KPASTERISK,
                ("F1", _) => KEY_F1,
                ("F2", _) => KEY_F2,
                ("F3", _) => KEY_F3,
                ("F4", _) => KEY_F4,
                ("F5", _) => KEY_F5,
                ("F6", _) => KEY_F6,
                ("F7", _) => KEY_F7,
                ("F8", _) => KEY_F8,
                ("F9", _) => KEY_F9,
                ("F10", _) => KEY_F10,
                ("F11", _) => KEY_F11,
                ("F12", _) => KEY_F12,
                ("F13", _) => KEY_F13,
                ("F14", _) => KEY_F14,
                ("F15", _) => KEY_F15,
                ("F16", _) => KEY_F16,
                ("F17", _) => KEY_F17,
                ("F18", _) => KEY_F18,
                ("F19", _) => KEY_F19,
                ("F20", _) => KEY_F20,
                ("F21", _) => KEY_F21,
                ("F22", _) => KEY_F22,
                ("F23", _) => KEY_F23,
                ("F24", _) => KEY_F24,
                ("NumLock", _) => KEY_NUMLOCK,
                ("ScrollLock", _) => KEY_SCROLLLOCK,
                ("Numpad0", _) => KEY_KP0,
                ("Numpad1", _) => KEY_KP1,
                ("Numpad2", _) => KEY_KP2,
                ("Numpad3", _) => KEY_KP3,
                ("Numpad4", _) => KEY_KP4,
                ("Numpad5", _) => KEY_KP5,
                ("Numpad6", _) => KEY_KP6,
                ("Numpad7", _) => KEY_KP7,
                ("Numpad8", _) => KEY_KP8,
                ("Numpad9", _) => KEY_KP9,
                ("NumpadSubtract", _) => KEY_KPMINUS,
                ("NumpadAdd", _) => KEY_KPPLUS,
                // ("NumpadDecimal", _) => ?,
                ("IntlBackslash", _) => KEY_102ND,
                ("IntlRo", _) => KEY_RO,
                ("NumpadEnter", _) => KEY_KPENTER,
                ("NumpadDivide", _) => KEY_KPSLASH,
                ("NumpadEqual", _) => KEY_KPEQUAL,
                ("NumpadComma", _) => KEY_KPCOMMA,
                ("NumpadParenLeft", _) => KEY_KPLEFTPAREN,
                ("NumpadParenRight", _) => KEY_KPRIGHTPAREN,
                // ("NumpadChangeSign", _) => ?,
                // ("Convert", _) => ?,
                ("KanaMode", _) => KEY_KATAKANA,
                // ("NonConvert", _) => ?,
                ("PrintScreen", _) => KEY_SYSRQ,
                ("Home", _) => KEY_HOME,
                ("ArrowUp", _) => KEY_UP,
                ("PageUp", _) => KEY_PAGEUP,
                ("ArrowLeft", _) => KEY_LEFT,
                ("ArrowRight", _) => KEY_RIGHT,
                ("End", _) => KEY_END,
                ("ArrowDown", _) => KEY_DOWN,
                ("PageDown", _) => KEY_PAGEDOWN,
                ("Insert", _) => KEY_INSERT,
                ("Delete", _) => KEY_DELETE,
                ("VolumeMute", _) | ("AudioVolumeMute", _) => KEY_MUTE,
                ("VolumeDown", _) | ("AudioVolumeDown", _) => KEY_VOLUMEDOWN,
                ("VolumeUp", _) | ("AudioVolumeUp", _) => KEY_VOLUMEUP,
                ("Pause", _) => KEY_PAUSE,

                ("Lang1", _) => KEY_HANGUEL,
                ("Lang2", _) => KEY_HANJA,
                ("IntlYen", _) => KEY_YEN,
                ("OSLeft", _) => KEY_LEFTMETA,
                ("OSRight", _) => KEY_RIGHTMETA,
                ("ContextMenu", _) => KEY_MENU,
                // ("BrowserStop", _) => ?,
                ("Cancel", _) => KEY_CANCEL,
                ("Again", _) => KEY_AGAIN,
                ("Props", _) => KEY_PROPS,
                ("Undo", _) => KEY_UNDO,
                // ("Select", _) => ?,
                ("Copy", _) => KEY_COPY,
                ("Open", _) => KEY_OPEN,
                ("Paste", _) => KEY_PASTE,
                ("Find", _) => KEY_FIND,
                ("Cut", _) => KEY_CUT,
                ("Help", _) => KEY_HELP,
                // ("LaunchApp2", _) => ?,
                // ("LaunchApp1", _) => ,
                ("LaunchMail", _) => KEY_MAIL,
                // ("BrowserFavorites", _) => ?,
                // ("BrowserBack", _) => ?,
                // ("BrowserForward", _) => ?,
                ("Eject", _) => KEY_EJECTCD,
                ("MediaTrackNext", _) => KEY_NEXTSONG,
                ("MediaPlayPause", _) => KEY_PLAYPAUSE,
                ("MediaTrackPrevious", _) => KEY_PREVIOUSSONG,
                ("MediaStop", _) => KEY_STOPCD,
                ("MediaSelect", _) | ("LaunchMediaPlayer", _) => KEY_MEDIA,
                // ("BrowserHome", _) => ?,
                // ("BrowserRefresh", _) => ?,
                // ("BrowserSearch", _) => ?,
                ("Power", _) => KEY_POWER,
                ("Sleep", _) => KEY_SLEEP,
                ("WakeUp", _) => KEY_WAKEUP,
                ("ControlLeft", _) => KEY_LEFTCTRL,
                ("ControlRight", _) => KEY_RIGHTCTRL,
                ("AltLeft", _) => KEY_LEFTALT,
                ("AltRight", _) => KEY_RIGHTALT,
                ("MetaLeft", _) => KEY_LEFTMETA,
                ("MetaRight", _) => KEY_RIGHTMETA,
                ("ShiftLeft", _) => KEY_LEFTSHIFT,
                ("ShiftRight", _) => KEY_RIGHTSHIFT,
                _ => KEY_UNKNOWN,
            }
        }

        let key_code: c_int = map_key(&event.code, &event.location);
        let state: c_int = match event.event_type {
            KeyboardEventType::UP => 0,
            KeyboardEventType::DOWN => 1,
            KeyboardEventType::REPEAT => 2,
        };

        if key_code == KEY_UNKNOWN {
            if let KeyboardEventType::DOWN = event.event_type {
                if !event.key.is_empty() {
                    // If the key is unknow try inserting the unicode character directly
                    // to do so use CTRL + SHIFT + U + UTF16 HEX of the unicode point.
                    let unicode_keys = event
                        .key
                        .encode_utf16()
                        .map(|b| format!("{:X}", b))
                        .collect::<Vec<String>>()
                        .concat();

                    debug!(
                        "Got unknown key: {} code: {}, trying to insert unicode using ctrl + \
                        shift + u + {}!",
                        event.code, event.key, unicode_keys
                    );

                    self.send(self.keyboard_fd, ET_KEY, KEY_LEFTCTRL, 1);
                    self.send(self.keyboard_fd, ET_KEY, KEY_LEFTSHIFT, 1);
                    self.send(self.keyboard_fd, ET_KEY, KEY_U, 1);
                    self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                    for c in unicode_keys.chars() {
                        let key_code = if c.is_alphabetic() {
                            map_key(&format!("Key{}", c), &KeyboardLocation::STANDARD)
                        } else {
                            map_key(&format!("Digit{}", c), &KeyboardLocation::STANDARD)
                        };

                        self.send(self.keyboard_fd, ET_KEY, key_code, 1);
                        self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                        self.send(self.keyboard_fd, ET_KEY, key_code, 0);
                        self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                    }
                    self.send(self.keyboard_fd, ET_KEY, KEY_LEFTCTRL, 0);
                    self.send(self.keyboard_fd, ET_KEY, KEY_LEFTSHIFT, 0);
                    self.send(self.keyboard_fd, ET_KEY, KEY_U, 0);
                    self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
                }
            } else {
                debug!(
                    "Got unknow key: code: {} key: {}, ignoring event.",
                    event.code, event.key
                );
            }
            return;
        }

        if event.ctrl {
            self.send(self.keyboard_fd, ET_KEY, KEY_LEFTCTRL, state);
            self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
        }
        if event.alt {
            self.send(self.keyboard_fd, ET_KEY, KEY_LEFTALT, state);
            self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
        }
        if event.meta {
            self.send(self.keyboard_fd, ET_KEY, KEY_LEFTMETA, state);
            self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
        }
        if event.shift {
            self.send(self.keyboard_fd, ET_KEY, KEY_LEFTSHIFT, state);
            self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
        }

        self.send(self.keyboard_fd, ET_KEY, key_code, state);
        self.send(self.keyboard_fd, ET_SYNC, EC_SYNC_REPORT, 0);
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable;
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::UInputDevice
    }
}
