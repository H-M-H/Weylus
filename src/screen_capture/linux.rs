use std::os::raw::{c_int, c_uint, c_void};
use std::slice::from_raw_parts;

use tracing::{trace, warn};

use crate::cerror::CError;
use crate::screen_capture::ScreenCapture;
use crate::x11helper::Capturable;

extern "C" {
    fn start_capture(handle: *const c_void, ctx: *mut c_void, err: *mut CError) -> *mut c_void;
    fn capture_sceen(
        handle: *mut c_void,
        img: *mut CImage,
        capture_cursor: c_int,
        err: *mut CError,
    );
    fn stop_capture(handle: *mut c_void, err: *mut CError);
}

#[repr(C)]
struct CImage {
    data: *const u8,
    width: c_uint,
    height: c_uint,
}

impl CImage {
    pub fn new() -> Self {
        Self {
            data: std::ptr::null(),
            width: 0,
            height: 0,
        }
    }

    pub fn size(&self) -> usize {
        (self.width * self.height * 4) as usize
    }

    pub fn data(&self) -> &[u8] {
        unsafe { from_raw_parts(self.data, self.size()) }
    }
}

pub struct ScreenCaptureX11 {
    handle: *mut c_void,
    img: CImage,
    capture_cursor: bool,
}

impl ScreenCaptureX11 {
    pub fn new(mut capture: Capturable, capture_cursor: bool) -> Result<Self, CError> {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        let handle = unsafe { start_capture(capture.handle(), std::ptr::null_mut(), &mut err) };
        fltk::app::unlock();
        if err.is_err() {
            Err(err)
        } else {
            Ok(Self {
                handle,
                img: CImage::new(),
                capture_cursor,
            })
        }
    }
}

impl Drop for ScreenCaptureX11 {
    fn drop(&mut self) {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        unsafe {
            stop_capture(self.handle, &mut err);
        }
        fltk::app::unlock();
    }
}

impl ScreenCapture for ScreenCaptureX11 {
    fn capture(&mut self) {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        unsafe {
            capture_sceen(
                self.handle,
                &mut self.img,
                self.capture_cursor.into(),
                &mut err,
            );
        }
        fltk::app::unlock();
        if err.is_err() {
            if err.code() == 1 {
                warn!("Failed to capture screen: {}", err);
            } else {
                trace!("Failed to capture screen: {}", err);
            }
        }
    }

    fn pixel_provider(&self) -> crate::video::PixelProvider {
        crate::video::PixelProvider::BGRA(self.img.data())
    }

    fn size(&self) -> (usize, usize) {
        (self.img.width as usize, self.img.height as usize)
    }
}
