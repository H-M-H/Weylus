// References:
//
// 1. MacOS Declarations, see /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/System/Library/Frameworks/IOKit.framework/Headers
//   IOKit/hidsystem/IOLLEvent.h
//
// 2. GitHub project "TabletMagic"
//    https://github.com/thinkyhead/TabletMagic/blob/master/daemon/SerialDaemon.cpp

use core_graphics::base::CGFloat;
use core_graphics::geometry::CGPoint;
use core_graphics::event::*;
use core_graphics::event_source::CGEventSourceStateID::HIDSystemState;
use core_graphics::event_source::CGEventSource;

use autopilot::geometry::Point;
use autopilot::mouse::MouseError;

pub const NX_SUBTYPE_DEFAULT: i64 = 0;
pub const NX_SUBTYPE_TABLET_POINT: i64 = 1;
pub const NX_SUBTYPE_TABLET_PROXIMITY: i64 = 2;
pub const NX_SUBTYPE_MOUSE_TOUCH: i64 = 3;

const NX_TABLET_POINTER_UNKNOWN: i64 = 0;
const NX_TABLET_POINTER_PEN: i64 = 1;
const NX_TABLET_POINTER_CURSOR: i64 = 2;
const NX_TABLET_POINTER_ERASER: i64 = 3;

const NX_TABLET_CAPABILITY_DEVICEIDMASK: i64 = 0x0001;
const NX_TABLET_CAPABILITY_ABSXMASK: i64 = 0x0002;
const NX_TABLET_CAPABILITY_ABSYMASK: i64 = 0x0004;
const NX_TABLET_CAPABILITY_VENDOR1MASK: i64 = 0x0008;
const NX_TABLET_CAPABILITY_VENDOR2MASK: i64 = 0x0010;
const NX_TABLET_CAPABILITY_VENDOR3MASK: i64 = 0x0020;
const NX_TABLET_CAPABILITY_BUTTONSMASK: i64 = 0x0040;
const NX_TABLET_CAPABILITY_TILTXMASK: i64 = 0x0080;
const NX_TABLET_CAPABILITY_TILTYMASK: i64 = 0x0100;
const NX_TABLET_CAPABILITY_ABSZMASK: i64 = 0x0200;
const NX_TABLET_CAPABILITY_PRESSUREMASK: i64 = 0x0400;
const NX_TABLET_CAPABILITY_TANGENTIALPRESSUREMASK: i64 = 0x0800;
const NX_TABLET_CAPABILITY_ORIENTINFOMASK: i64 = 0x1000;
const NX_TABLET_CAPABILITY_ROTATIONMASK: i64 = 0x2000;

// mock
const MOCK_VENDOR_UNIQUE_ID: i64 = 0xdeadbeef;
const MOCK_DEVICE_ID: i64 = 0x81; // just a single device for now
fn populate_tablet_point_event(event: &CGEvent, buttons: i64, point: Point, pressure: f64) {
    // note:
    // 1. assume subtype already set.

    // CGEventSetIntegerValueField(move1, kCGTabletEventPointX, stylus.point.x);
    // CGEventSetIntegerValueField(move1, kCGTabletEventPointY, stylus.point.y);
    // CGEventSetIntegerValueField(move1, kCGTabletEventPointButtons, 0x0000);
    // CGEventSetDoubleValueField(move1, kCGTabletEventPointPressure, stylus.pressure / PRESSURE_SCALE);
    // CGEventSetDoubleValueField(move1, kCGTabletEventTiltX, stylus.tilt.x);
    // CGEventSetDoubleValueField(move1, kCGTabletEventTiltY, stylus.tilt.y);
    // CGEventSetIntegerValueField(move1, kCGTabletEventDeviceID, stylus.proximity.deviceID);
    // CGEventSetIntegerValueField(move1, kCGTabletEventPointZ, 0);
    // CGEventSetDoubleValueField(move1, kCGTabletEventRotation, 0);
    // CGEventSetDoubleValueField(move1, kCGTabletEventTangentialPressure, 0);

    event.set_integer_value_field(EventField::TABLET_EVENT_POINT_X, point.x as i64);
    event.set_integer_value_field(EventField::TABLET_EVENT_POINT_Y, point.y as i64);
    event.set_integer_value_field(EventField::TABLET_EVENT_POINT_BUTTONS, buttons);
    event.set_double_value_field(EventField::TABLET_EVENT_POINT_PRESSURE, pressure);
    event.set_double_value_field(EventField::TABLET_EVENT_TILT_X, 0.0);     // tilt is yet zero
    event.set_double_value_field(EventField::TABLET_EVENT_TILT_Y, 0.0);
    event.set_integer_value_field(EventField::TABLET_EVENT_DEVICE_ID, MOCK_DEVICE_ID);
    event.set_integer_value_field(EventField::TABLET_EVENT_POINT_Z, 0);
    event.set_double_value_field(EventField::TABLET_EVENT_ROTATION, 0.0);  // yet not rotated
    event.set_double_value_field(EventField::TABLET_EVENT_TANGENTIAL_PRESSURE, 0.0);

    event.set_double_value_field(EventField::MOUSE_EVENT_PRESSURE, pressure);
}

fn populate_tablet_proximity_event(event: &CGEvent, enter_tablet: bool, is_enter_eraser: bool) {
    // note:
    // 1. assume subtype already set.

    event.set_integer_value_field(
        EventField::TABLET_PROXIMITY_EVENT_ENTER_PROXIMITY,
        enter_tablet as i64,
    );
    event.set_integer_value_field(
        EventField::TABLET_PROXIMITY_EVENT_POINTER_TYPE,
        match is_enter_eraser {
            true => NX_TABLET_POINTER_ERASER,
            false => NX_TABLET_POINTER_PEN,
        },
    );

    event.set_integer_value_field(EventField::TABLET_PROXIMITY_EVENT_VENDOR_ID, 0xbeef); // A made-up Vendor ID (Wacom's is 0x056A)
    event.set_integer_value_field(EventField::TABLET_PROXIMITY_EVENT_TABLET_ID, 1);
    event.set_integer_value_field(EventField::TABLET_PROXIMITY_EVENT_DEVICE_ID, MOCK_DEVICE_ID);
    event.set_integer_value_field(EventField::TABLET_PROXIMITY_EVENT_POINTER_ID, 0);
    event.set_integer_value_field(EventField::TABLET_PROXIMITY_EVENT_SYSTEM_TABLET_ID, 0);
    event.set_integer_value_field(
        EventField::TABLET_PROXIMITY_EVENT_VENDOR_POINTER_TYPE,
        0x0802,
    ); // basic stylus
    event.set_integer_value_field(
        EventField::TABLET_PROXIMITY_EVENT_VENDOR_POINTER_SERIAL_NUMBER,
        1,
    );
    event.set_integer_value_field(
        EventField::TABLET_PROXIMITY_EVENT_VENDOR_UNIQUE_ID,
        MOCK_VENDOR_UNIQUE_ID,
    );

    // Indicate which fields in the point event contain valid data. This allows
    // applications to handle devices with varying capabilities.
    let capability_mask = NX_TABLET_CAPABILITY_DEVICEIDMASK
        | NX_TABLET_CAPABILITY_ABSXMASK
        | NX_TABLET_CAPABILITY_ABSYMASK
        | NX_TABLET_CAPABILITY_BUTTONSMASK
        | NX_TABLET_CAPABILITY_TILTXMASK
        | NX_TABLET_CAPABILITY_TILTYMASK
        | NX_TABLET_CAPABILITY_PRESSUREMASK;
    //      |   NX_TABLET_CAPABILITY_TANGENTIALPRESSUREMASK
    //      |   NX_TABLET_CAPABILITY_ORIENTINFOMASK
    //      |   NX_TABLET_CAPABILITY_ROTATIONMASK

    event.set_integer_value_field(
        EventField::TABLET_PROXIMITY_EVENT_CAPABILITY_MASK,
        capability_mask,
    );
}


#[derive(Debug, PartialEq, Eq)]
pub enum MacosPenEventType {
    Move,
    Down,
    Up,
    Enter, // proximity enter
    Leave, // proximity leave
}

const ERASER_BUTTON: i64 = 32;

#[cfg(target_os = "macos")]
pub fn macos_send_tablet_event(
    point: Point,
    pe_type: MacosPenEventType,
    button: i64,
    buttons: i64,
    pressure: f64,
) -> Result<(), MouseError> {


    let make_event = |event_type: CGEventType| {
        let source = CGEventSource::new(HIDSystemState).unwrap();
        let event = CGEvent::new_mouse_event(
            source,
            event_type,
            CGPoint::new(point.x as CGFloat, point.y as CGFloat),
            CGMouseButton::Left,
        );

        return event;
    };

    // let is_eraser = button == ERASER_BUTTON || (buttons & ERASER_BUTTON) != 0;

    match pe_type {
        MacosPenEventType::Enter | MacosPenEventType::Leave => {
            // send proximity event
            let event = make_event(CGEventType::MouseMoved).unwrap();
            event.set_type(CGEventType::TabletProximity);
            event.set_integer_value_field(
                EventField::MOUSE_EVENT_SUB_TYPE,
                NX_SUBTYPE_TABLET_PROXIMITY,
            );
            populate_tablet_proximity_event(&event, pe_type == MacosPenEventType::Enter, false);

            event.post(CGEventTapLocation::HID);
        },
        _ => {
            // then send a MouseMoved event
            let event_type: CGEventType = match pe_type {
                MacosPenEventType::Down => match button {
                    1 => CGEventType::LeftMouseDown,
                    2 => CGEventType::RightMouseDown,
                    _ => CGEventType::OtherMouseDown, // eg: 32 for eraser button
                },
                MacosPenEventType::Up => match button {
                    1 => CGEventType::LeftMouseUp,
                    2 => CGEventType::RightMouseUp,
                    _ => CGEventType::OtherMouseUp,
                },
                _ => match buttons {
                    0 => CGEventType::MouseMoved,
                    1 => CGEventType::LeftMouseDragged,
                    2 => CGEventType::RightMouseDragged,
                    _ => CGEventType::OtherMouseDragged,
                },
            };

            let event = make_event(event_type).unwrap();
            event.set_double_value_field(EventField::MOUSE_EVENT_PRESSURE, pressure);
            event.set_integer_value_field(
                EventField::MOUSE_EVENT_SUB_TYPE,
                NX_SUBTYPE_TABLET_POINT,
            );
            populate_tablet_proximity_event(&event, true, false);
            populate_tablet_point_event(&event, buttons, point, pressure);
            event.post(CGEventTapLocation::HID);
        },
    }

    // if pe_type == PressureEventType::Down || pe_type == PressureEventType::Up {
    //     // when pointer down or up, use TabletProximity event to notify the OS
    // } else {
    //     // regular daily notify

    //     let event = make_event(CGEventType::MouseMoved).unwrap();
    //     event.set_type(CGEventType::TabletPointer);
    //     event.set_integer_value_field(EventField::MOUSE_EVENT_SUB_TYPE, 1); // 1 for https://developer.apple.com/documentation/coregraphics/cgeventmousesubtype/tabletpoint
    //     populate_tablet_point_event(&event, point, pressure);

    //     event.post(CGEventTapLocation::HID);
    // }

    Ok(())
}
