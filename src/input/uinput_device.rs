use std::os::raw::c_int;

use crate::input::pointer::PointerDevice;
use crate::protocol::PointerEvent;
use crate::protocol::PointerEventType;
use crate::protocol::PointerType;
use crate::x11helper::WindowInfo;

use crate::cerror::CError;

use tracing::{info, warn};

extern "C" {
    fn init_uinput_pointer(err: *mut CError) -> c_int;
    fn init_uinput_multitouch(err: *mut CError) -> c_int;
    fn destroy_uinput_device(fd: c_int);
    fn send_uinput_event(device: c_int, typ: c_int, code: c_int, value: c_int, err: *mut CError);
}

struct MultiTouch {
    id: i64,
}

pub struct GraphicTablet {
    pointer_fd: c_int,
    multitouch_fd: c_int,
    multi_touches: [Option<MultiTouch>; 5],
    winfo: WindowInfo,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl GraphicTablet {
    pub fn new(winfo: WindowInfo) -> Result<Self, CError> {
        let mut err = CError::new();
        let pointer_fd = unsafe { init_uinput_pointer(&mut err) };
        if err.is_err() {
            return Err(err);
        }
        /*let multitouch_fd = unsafe { init_uinput_multitouch(&mut err) };
        if err.is_err() {
            return Err(err);
        }*/
        let tblt = Self {
            pointer_fd: pointer_fd,
            multitouch_fd: 0, // multitouch_fd,
            multi_touches: Default::default(),
            winfo: winfo,
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        Ok(tblt)
    }

    fn transform_x(&self, x: f64) -> i32 {
        let x = (x * self.width + self.x) * 65535.0;
        x as i32
    }

    fn transform_y(&self, y: f64) -> i32 {
        let y = (y * self.height + self.y) * 65535.0;
        y as i32
    }

    fn transform_pressure(&self, p: f64) -> i32 {
        (p * 65535.0) as i32
    }

    fn transform_touch_size(&self, s: f64) -> i32 {
        (s * 65535.0) as i32
    }

    fn find_slot(&self, id: i64) -> Option<usize> {
        self.multi_touches
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

    fn send(&self, typ: c_int, code: c_int, value: c_int) {
        let mut err = CError::new();
        unsafe {
            send_uinput_event(self.pointer_fd, typ, code, value, &mut err);
        }
        if err.is_err() {
            warn!("{}", err);
        }
    }

    fn send_touch(&self, typ: c_int, code: c_int, value: c_int) {
        let mut err = CError::new();
        unsafe {
            send_uinput_event(self.multitouch_fd, typ, code, value, &mut err);
        }
        if err.is_err() {
            warn!("{}", err);
        }
    }
}

impl Drop for GraphicTablet {
    fn drop(&mut self) {
        unsafe {
            destroy_uinput_device(self.pointer_fd);
            // destroy_uinput_device(self.multitouch_fd);
        };
    }
}

// Event Types
const ET_SYNC: c_int = 0x00;
const ET_KEY: c_int = 0x01;
const ET_RELATIVE: c_int = 0x02;
const ET_ABSOLUTE: c_int = 0x03;

// Event Codes
const EC_SYNC_REPORT: c_int = 1;
const EC_SYNC_MT_REPORT: c_int = 2;

const EC_KEY_MOUSE_LEFT: c_int = 0x110;
const EC_KEY_TOOL_PEN: c_int = 0x140;
const EC_KEY_TOUCH: c_int = 0x14a;
const EC_KEY_STYLUS: c_int = 0x14b;
const EC_KEY_TOOL_FINGER: c_int = 0x145;
const EC_KEY_TOOL_DOUBLETAP: c_int = 0x14d;
const EC_KEY_TOOL_TRIPLETAP: c_int = 0x14e;
const EC_KEY_TOOL_QUADTAP: c_int = 0x14f; /* Four fingers on trackpad */
const EC_KEY_TOOL_QUINTTAP: c_int = 0x148; /* Five fingers on trackpad */

const EC_RELATIVE_X: c_int = 0x00;
const EC_RELATIVE_Y: c_int = 0x01;

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

// Maximum for Absolute Values
const ABS_MAX: c_int = 65535;

impl PointerDevice for GraphicTablet {
    fn send_event(&mut self, event: &PointerEvent) {
        let geometry = self.winfo.geometry().unwrap();
        self.x = geometry.x;
        self.y = geometry.y;
        self.width = geometry.width;
        self.height = geometry.height;
        match event.pointer_type {
            /*PointerType::Touch => {
                match event.event_type {
                    PointerEventType::DOWN | PointerEventType::MOVE => {
                        let slot: usize;
                        // check if this event is already assigned to one of our 10 multitouch slots
                        if let Some(s) = self.find_slot(event.pointer_id) {
                            slot = s;
                        } else {
                            // this event is not assigned to a slot, lets try to do so now
                            // find the first unused slot
                            if let Some(s) = self.multi_touches.iter().enumerate().find_map(
                                |(slot, mt)| match mt {
                                    None => Some(slot),
                                    Some(_) => None,
                                },
                            ) {
                                slot = s;
                                self.multi_touches[slot] = Some(MultiTouch {
                                    id: event.pointer_id,
                                })
                            } else {
                                // out of slots, do nothing
                                return;
                            }
                        };
                        self.send_touch(ET_ABSOLUTE, EC_ABS_MT_SLOT, slot as i32);
                        self.send_touch(ET_ABSOLUTE, EC_ABS_MT_TRACKING_ID, slot as i32);
                        self.send_touch(
                            ET_ABSOLUTE,
                            EC_ABS_MT_POSITION_X,
                            self.transform_x(event.x),
                        );
                        self.send_touch(
                            ET_ABSOLUTE,
                            EC_ABS_MT_POSITION_Y,
                            self.transform_y(event.y),
                        );
                        self.send_touch(
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
                        self.send_touch(ET_ABSOLUTE, EC_ABS_MT_TOUCH_MAJOR, major);
                        self.send_touch(ET_ABSOLUTE, EC_ABS_MT_TOUCH_MINOR, minor);
                        self.send_touch(ET_ABSOLUTE, EC_ABS_MT_ORIENTATION, orientation);
                        self.send_touch(ET_ABSOLUTE, EC_ABSOLUTE_X, self.transform_x(event.x));
                        self.send_touch(ET_ABSOLUTE, EC_ABSOLUTE_Y, self.transform_x(event.y));
                        self.send_touch(ET_KEY, EC_KEY_TOUCH, 1);
                        self.send_touch(ET_KEY, EC_KEY_TOOL_FINGER, 1);
                        self.send_touch(ET_SYNC, EC_SYNC_REPORT, 1);
                        for (i, mt) in self.multi_touches.iter().enumerate() {
                            info!(
                                "slot: {} id: {}",
                                i,
                                if let Some(mt) = mt {
                                    mt.id.to_string()
                                } else {
                                    "".to_string()
                                }
                            );
                        }
                    }
                    PointerEventType::CANCEL | PointerEventType::UP => {
                        // remove from slot
                        if let Some(slot) = self.find_slot(event.pointer_id) {
                            self.send_touch(ET_ABSOLUTE, EC_ABS_MT_SLOT, slot as i32);
                            self.send_touch(ET_ABSOLUTE, EC_ABS_MT_TRACKING_ID, -1);
                            self.multi_touches[slot] = None;
                        }
                    }
                };
            }*/
            PointerType::Pen => {
                self.send(ET_ABSOLUTE, EC_ABSOLUTE_X, self.transform_x(event.x));
                self.send(ET_ABSOLUTE, EC_ABSOLUTE_Y, self.transform_y(event.y));
                self.send(
                    ET_ABSOLUTE,
                    EC_ABSOLUTE_PRESSURE,
                    self.transform_pressure(event.pressure),
                );
                self.send(ET_ABSOLUTE, EC_ABSOLUTE_TILT_X, event.tilt_x);
                self.send(ET_ABSOLUTE, EC_ABSOLUTE_TILT_Y, event.tilt_y);
                match event.event_type {
                    PointerEventType::DOWN => {
                        self.send(ET_KEY, EC_KEY_TOOL_PEN, 1);
                    }
                    PointerEventType::UP | PointerEventType::CANCEL => {
                        self.send(ET_KEY, EC_KEY_TOOL_PEN, 0);
                    }
                    PointerEventType::MOVE => (),
                }
                self.send(ET_SYNC, EC_SYNC_REPORT, 1);
            }
            PointerType::Mouse | PointerType::Unknown | PointerType::Touch => {
                if !event.is_primary {
                    return;
                }
                self.send(ET_ABSOLUTE, EC_ABSOLUTE_X, self.transform_x(event.x));
                self.send(ET_ABSOLUTE, EC_ABSOLUTE_Y, self.transform_y(event.y));
                self.send(
                    ET_ABSOLUTE,
                    EC_ABSOLUTE_PRESSURE,
                    self.transform_pressure(1.0),
                );
                match event.event_type {
                    PointerEventType::DOWN => self.send(ET_KEY, EC_KEY_MOUSE_LEFT, 1),
                    PointerEventType::MOVE => (),
                    _ => self.send(ET_KEY, EC_KEY_MOUSE_LEFT, 0),
                }
                self.send(ET_SYNC, EC_SYNC_REPORT, 1);
            }
        }
    }
}
