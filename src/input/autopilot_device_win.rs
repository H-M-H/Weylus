use autopilot::mouse;
use autopilot::mouse::ScrollDirection;

use winapi::shared::windef::{HWND, POINT, RECT};
use winapi::um::winuser::*;

use tracing::warn;

use crate::input::autopilot_device::AutoPilotDevice;
use crate::input::device::{InputDevice, InputDeviceType};
use crate::protocol::{
    Button, KeyboardEvent, PointerEvent, PointerEventType, PointerType, WheelEvent,
};

use crate::capturable::{Capturable, Geometry};

pub struct WindowsInput {
    capturable: Box<dyn Capturable>,
    autopilot_device: AutoPilotDevice,
    pointer_device_handle: *mut HSYNTHETICPOINTERDEVICE__,
    touch_device_handle: *mut HSYNTHETICPOINTERDEVICE__,
}

impl WindowsInput {
    pub fn new(capturable: Box<dyn Capturable>) -> Self {
        unsafe {
            Self {
                capturable: capturable.clone(),
                autopilot_device: AutoPilotDevice::new(capturable.clone()),
                pointer_device_handle: CreateSyntheticPointerDevice(PT_PEN, 1, 1),
                touch_device_handle: CreateSyntheticPointerDevice(PT_TOUCH, 5, 1),
            }
        }
    }
}

impl InputDevice for WindowsInput {
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
        let (offset_x, offset_y, width, height) = match self.capturable.geometry().unwrap() {
            Geometry::VirtualScreen(offset_x, offset_y, width, height) => {
                (offset_x, offset_y, width, height)
            }
            _ => unreachable!(),
        };
        let (x, y) = (
            (event.x * width as f64) as i32 + offset_x,
            (event.y * height as f64) as i32 + offset_y,
        );
        let mut pointer_type_info = POINTER_TYPE_INFO {
            type_: PT_PEN,
            u: unsafe { std::mem::zeroed() },
        };
        let pointer_flags;
        let button_change_type;
        match event.event_type {
            PointerEventType::DOWN => {
                pointer_flags = POINTER_FLAG_INRANGE
                    | POINTER_FLAG_INCONTACT
                    | POINTER_FLAG_PRIMARY
                    | POINTER_FLAG_DOWN
                    | POINTER_FLAG_UPDATE;
                button_change_type = POINTER_CHANGE_FIRSTBUTTON_DOWN;
            }

            PointerEventType::UP => {
                pointer_flags = POINTER_FLAG_PRIMARY | POINTER_FLAG_UP;
                button_change_type = POINTER_CHANGE_FIRSTBUTTON_UP;
            }
            PointerEventType::MOVE => {
                pointer_flags = POINTER_FLAG_INRANGE
                    | POINTER_FLAG_INCONTACT
                    | POINTER_FLAG_PRIMARY
                    | POINTER_FLAG_UPDATE;
                button_change_type = POINTER_CHANGE_NONE;
            }

            PointerEventType::CANCEL => {
                pointer_flags = POINTER_FLAG_PRIMARY | POINTER_FLAG_CANCELED;
                button_change_type = POINTER_CHANGE_NONE;
            }
        };
        match event.pointer_type {
            PointerType::Pen => {
                unsafe {
                    *pointer_type_info.u.penInfo_mut() = POINTER_PEN_INFO {
                        pointerInfo: POINTER_INFO {
                            pointerType: PT_PEN,
                            pointerId: event.pointer_id as u32,
                            frameId: 0,
                            pointerFlags: pointer_flags,
                            sourceDevice: 0 as *mut winapi::ctypes::c_void, //maybe use syntheticPointerDeviceHandle here but works with 0
                            hwndTarget: 0 as HWND,
                            ptPixelLocation: POINT { x: x, y: y },
                            ptHimetricLocation: POINT { x: 0, y: 0 },
                            ptPixelLocationRaw: POINT { x: x, y: y },
                            ptHimetricLocationRaw: POINT { x: 0, y: 0 },
                            dwTime: 0,
                            historyCount: 1,
                            InputData: 0,
                            dwKeyStates: 0,
                            PerformanceCount: 0,
                            ButtonChangeType: button_change_type,
                        },
                        penFlags: PEN_FLAG_NONE,
                        penMask: PEN_MASK_PRESSURE
                            | PEN_MASK_ROTATION
                            | PEN_MASK_TILT_X
                            | PEN_MASK_TILT_Y,
                        pressure: (event.pressure * 100f64) as u32,
                        rotation: event.twist as u32,
                        tiltX: event.tilt_x,
                        tiltY: event.tilt_y,
                    };
                    InjectSyntheticPointerInput(self.pointer_device_handle, &pointer_type_info, 1);
                }
            }
            PointerType::Touch => {
                unsafe {
                    *pointer_type_info.u.touchInfo_mut() = POINTER_TOUCH_INFO {
                        pointerInfo: POINTER_INFO {
                            pointerType: PT_TOUCH,
                            pointerId: event.pointer_id as u32,
                            frameId: 0,
                            pointerFlags: pointer_flags,
                            sourceDevice: self.touch_device_handle as *mut winapi::ctypes::c_void, //maybe use syntheticPointerDeviceHandle here but works with 0
                            hwndTarget: 0 as HWND,
                            ptPixelLocation: POINT { x: x, y: y },
                            ptHimetricLocation: POINT { x: 0, y: 0 },
                            ptPixelLocationRaw: POINT { x: x, y: y },
                            ptHimetricLocationRaw: POINT { x: 0, y: 0 },
                            dwTime: 0,
                            historyCount: 1,
                            InputData: 0,
                            dwKeyStates: 0,
                            PerformanceCount: 0,
                            ButtonChangeType: button_change_type,
                        },
                        touchFlags: TOUCH_FLAG_NONE,
                        touchMask: TOUCH_MASK_PRESSURE,
                        orientation: 0,
                        pressure: (event.pressure * 100f64) as u32,
                        rcContact: RECT {
                            left: 0,
                            top: 0,
                            right: event.width as i32,
                            bottom: event.height as i32,
                        },
                        rcContactRaw: RECT {
                            left: 0,
                            top: 0,
                            right: event.width as i32,
                            bottom: event.height as i32,
                        },
                    };
                    InjectSyntheticPointerInput(self.touch_device_handle, &pointer_type_info, 1);
                }
            }
            PointerType::Mouse => {
                if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
                    event.x * width as f64,
                    event.y * height as f64,
                )) {
                    warn!("Could not move mouse: {}", err);
                }
                match event.button {
                    Button::PRIMARY => {
                        mouse::toggle(mouse::Button::Left, event.buttons.contains(event.button))
                    }
                    Button::AUXILARY => {
                        mouse::toggle(mouse::Button::Middle, event.buttons.contains(event.button))
                    }
                    Button::SECONDARY => {
                        mouse::toggle(mouse::Button::Right, event.buttons.contains(event.button))
                    }
                    _ => (),
                }
            }
            PointerType::Unknown => todo!(),
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        self.autopilot_device.send_keyboard_event(event);
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable;
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::WindowsInput
    }
}
