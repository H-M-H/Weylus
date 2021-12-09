use winapi::shared::minwindef::DWORD;
use winapi::shared::windef::{HWND, POINT};
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
            InitializeTouchInjection(5, TOUCH_FEEDBACK_DEFAULT);
            Self {
                capturable: capturable.clone(),
                autopilot_device: AutoPilotDevice::new(capturable),
                pointer_device_handle: CreateSyntheticPointerDevice(PT_PEN, 1, 1),
                touch_device_handle: CreateSyntheticPointerDevice(PT_TOUCH, 5, 1),
            }
        }
    }
}

impl InputDevice for WindowsInput {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        unsafe { mouse_event(MOUSEEVENTF_WHEEL, 0, 0, event.dy as DWORD, 0) };
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        let (offset_x, offset_y, width, height, left, top) =
            match self.capturable.geometry().unwrap() {
                Geometry::VirtualScreen(offset_x, offset_y, width, height, left, top) => {
                    (offset_x, offset_y, width, height, left, top)
                }
                _ => unreachable!(),
            };
        let (x, y) = (
            (event.x * width as f64) as i32 + offset_x,
            (event.y * height as f64) as i32 + offset_y,
        );
        let button_change_type = match event.buttons {
            Button::PRIMARY => POINTER_CHANGE_FIRSTBUTTON_DOWN,
            Button::SECONDARY => POINTER_CHANGE_SECONDBUTTON_DOWN,
            Button::AUXILARY => POINTER_CHANGE_THIRDBUTTON_DOWN,
            Button::NONE => POINTER_CHANGE_NONE,
            _ => POINTER_CHANGE_NONE,
        };
        let mut pointer_flags = match event.event_type {
            PointerEventType::DOWN => {
                POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN
            }
            PointerEventType::MOVE => {
                POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_UPDATE
            }
            PointerEventType::UP => POINTER_FLAG_UP,
            PointerEventType::CANCEL => {
                POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE | POINTER_FLAG_CANCELED
            }
        };
        if event.is_primary {
            pointer_flags |= POINTER_FLAG_PRIMARY;
        }
        match event.pointer_type {
            PointerType::Pen => {
                unsafe {
                    let mut pointer_type_info = POINTER_TYPE_INFO {
                        type_: PT_PEN,
                        u: std::mem::zeroed(),
                    };
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
                        pressure: (event.pressure * 1024f64) as u32,
                        rotation: event.twist as u32,
                        tiltX: event.tilt_x,
                        tiltY: event.tilt_y,
                    };
                    InjectSyntheticPointerInput(self.pointer_device_handle, &pointer_type_info, 1);
                }
            }
            PointerType::Touch => {
                unsafe {
                    let mut pointer_type_info = POINTER_TYPE_INFO {
                        type_: PT_TOUCH,
                        u: std::mem::zeroed(),
                    };

                    let mut pointer_touch_info: POINTER_TOUCH_INFO = std::mem::zeroed();
                    pointer_touch_info.pointerInfo = std::mem::zeroed();
                    pointer_touch_info.pointerInfo.pointerType = PT_TOUCH;
                    pointer_touch_info.pointerInfo.pointerFlags = pointer_flags;
                    pointer_touch_info.pointerInfo.pointerId = event.pointer_id as u32; //event.pointer_id as u32; Using the actual pointer id causes errors in the touch injection
                    pointer_touch_info.pointerInfo.ptPixelLocation = POINT { x, y };
                    pointer_touch_info.touchFlags = TOUCH_FLAG_NONE;
                    pointer_touch_info.touchMask = TOUCH_MASK_PRESSURE;
                    pointer_touch_info.pressure = (event.pressure * 1024f64) as u32;

                    pointer_touch_info.pointerInfo.ButtonChangeType = button_change_type;

                    *pointer_type_info.u.touchInfo_mut() = pointer_touch_info;
                    InjectSyntheticPointerInput(self.touch_device_handle, &pointer_type_info, 1);
                }
            }
            PointerType::Mouse => {
                let mut dw_flags = 0;

                let (screen_x, screen_y) = (
                    (event.x * width as f64) as i32 + left,
                    (event.y * height as f64) as i32 + top,
                );

                match event.event_type {
                    PointerEventType::DOWN => match event.buttons {
                        Button::PRIMARY => {
                            dw_flags |= MOUSEEVENTF_LEFTDOWN;
                        }
                        Button::SECONDARY => {
                            dw_flags |= MOUSEEVENTF_RIGHTDOWN;
                        }
                        Button::AUXILARY => {
                            dw_flags |= MOUSEEVENTF_MIDDLEDOWN;
                        }
                        _ => {}
                    },
                    PointerEventType::MOVE => {
                        unsafe { SetCursorPos(screen_x, screen_y) };
                    }
                    PointerEventType::UP => match event.button {
                        Button::PRIMARY => {
                            dw_flags |= MOUSEEVENTF_LEFTUP;
                        }
                        Button::SECONDARY => {
                            dw_flags |= MOUSEEVENTF_RIGHTUP;
                        }
                        Button::AUXILARY => {
                            dw_flags |= MOUSEEVENTF_MIDDLEUP;
                        }
                        _ => {}
                    },
                    PointerEventType::CANCEL => {
                        dw_flags |= MOUSEEVENTF_LEFTUP;
                    }
                }
                unsafe { mouse_event(dw_flags, 0 as u32, 0 as u32, 0, 0) };
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
