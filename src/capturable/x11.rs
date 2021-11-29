use crate::capturable::{Capturable, Geometry, Recorder};
use crate::cerror::CError;
use crate::video::PixelProvider;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_int, c_uint, c_void};
use std::slice::from_raw_parts;
use std::sync::Arc;
use std::{error::Error, fmt};

use tracing::debug;

extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut c_void;
    fn XCloseDisplay(disp: *mut c_void) -> c_int;
    fn XInitThreads() -> c_int;
    fn XLockDisplay(disp: *mut c_void);
    fn XUnlockDisplay(disp: *mut c_void);

    fn x11_set_error_handler();

    fn create_capturables(
        disp: *mut c_void,
        handles: *mut *mut c_void,
        size: c_int,
        err: *mut CError,
    ) -> c_int;

    fn clone_capturable(handle: *const c_void) -> *mut c_void;
    fn destroy_capturable(handle: *mut c_void);
    fn get_capturable_name(handle: *const c_void) -> *const c_char;
    fn capturable_before_input(handle: *mut c_void, err: *mut CError);
    fn get_geometry_relative(
        handle: *const c_void,
        x: *mut c_float,
        y: *mut c_float,
        width: *mut c_float,
        height: *mut c_float,
        err: *mut CError,
    );

    fn map_input_device_to_entire_screen(
        disp: *mut c_void,
        device_name: *const c_char,
        libinput: c_int,
        err: *mut CError,
    );
    fn start_capture(handle: *const c_void, ctx: *mut c_void, err: *mut CError) -> *mut c_void;
    fn capture_screen(
        handle: *mut c_void,
        img: *mut CImage,
        capture_cursor: c_int,
        err: *mut CError,
    );
    fn stop_capture(handle: *mut c_void, err: *mut CError);
}

pub fn x11_init() {
    unsafe {
        XInitThreads();
        x11_set_error_handler();
    }
}

pub struct X11Capturable {
    handle: *mut c_void,
    // keep a reference to the display so it is not closed while a capturable still exists
    disp: Arc<XDisplay>,
}

impl Clone for X11Capturable {
    fn clone(&self) -> Self {
        let handle = unsafe { clone_capturable(self.handle) };
        Self {
            handle,
            disp: self.disp.clone(),
        }
    }
}

unsafe impl Send for X11Capturable {}

impl X11Capturable {
    pub unsafe fn handle(&mut self) -> *mut c_void {
        self.handle
    }
}

impl Capturable for X11Capturable {
    fn name(&self) -> String {
        unsafe {
            CStr::from_ptr(get_capturable_name(self.handle))
                .to_string_lossy()
                .into()
        }
    }

    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        let mut x: c_float = 0.0;
        let mut y: c_float = 0.0;
        let mut width: c_float = 0.0;
        let mut height: c_float = 0.0;
        let mut err = CError::new();
        self.disp.lock();
        unsafe {
            get_geometry_relative(
                self.handle,
                &mut x,
                &mut y,
                &mut width,
                &mut height,
                &mut err,
            );
        }
        self.disp.unlock();
        if err.is_err() {
            return Err(Box::new(err));
        }
        Ok(Geometry::Relative(
            x.into(),
            y.into(),
            width.into(),
            height.into(),
        ))
    }

    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        let mut err = CError::new();
        self.disp.lock();
        unsafe { capturable_before_input(self.handle, &mut err) };
        self.disp.unlock();
        if err.is_err() {
            Err(Box::new(err))
        } else {
            Ok(())
        }
    }

    fn recorder(&self, capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        match RecorderX11::new(self.clone(), capture_cursor) {
            Ok(recorder) => Ok(Box::new(recorder)),
            Err(err) => Err(Box::new(err)),
        }
    }
}

impl fmt::Display for X11Capturable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Drop for X11Capturable {
    fn drop(&mut self) {
        unsafe {
            destroy_capturable(self.handle);
        }
    }
}

struct XDisplay {
    handle: *mut c_void,
}

impl XDisplay {
    pub fn new() -> Option<Self> {
        let handle = unsafe { XOpenDisplay(std::ptr::null()) };
        if handle.is_null() {
            return None;
        }
        Some(Self { handle })
    }

    pub fn lock(&self) {
        unsafe { XLockDisplay(self.handle) }
    }

    pub fn unlock(&self) {
        unsafe { XUnlockDisplay(self.handle) }
    }
}

impl Drop for XDisplay {
    fn drop(&mut self) {
        self.lock();
        unsafe { XCloseDisplay(self.handle) };
        self.unlock();
    }
}

pub struct X11Context {
    disp: Arc<XDisplay>,
}

impl X11Context {
    pub fn new() -> Option<Self> {
        let disp = XDisplay::new()?;
        Some(Self {
            disp: Arc::new(disp),
        })
    }

    pub fn capturables(&mut self) -> Result<Vec<X11Capturable>, CError> {
        let mut err = CError::new();
        let mut handles = [std::ptr::null_mut::<c_void>(); 128];
        self.disp.lock();
        let size = unsafe {
            create_capturables(
                self.disp.handle,
                handles.as_mut_ptr(),
                handles.len() as c_int,
                &mut err,
            )
        };
        self.disp.unlock();
        if err.is_err() {
            if err.code() == 2 {
                debug!("{}", err);
            } else {
                return Err(err);
            }
        }
        Ok(handles[0..size as usize]
            .iter()
            .map(|handle| X11Capturable {
                handle: *handle,
                disp: self.disp.clone(),
            })
            .collect::<Vec<X11Capturable>>())
    }

    pub fn map_input_device_to_entire_screen(&mut self, device_name: &str, pen: bool) -> CError {
        let mut err = CError::new();
        let device_name_c_str = CString::new(device_name).unwrap();
        self.disp.lock();
        unsafe {
            map_input_device_to_entire_screen(
                self.disp.handle,
                device_name_c_str.as_ptr(),
                pen.into(),
                &mut err,
            )
        };
        self.disp.unlock();
        if err.is_err() {
            debug!("Failed to map input device to screen: {}", &err);
        }
        err
    }
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

pub struct RecorderX11 {
    handle: *mut c_void,
    // keep a reference to the capturable so it is not destroyed until we are done
    #[allow(dead_code)]
    capturable: X11Capturable,
    img: CImage,
    capture_cursor: bool,
}

impl RecorderX11 {
    pub fn new(mut capturable: X11Capturable, capture_cursor: bool) -> Result<Self, CError> {
        let mut err = CError::new();
        capturable.disp.lock();
        let handle = unsafe { start_capture(capturable.handle(), std::ptr::null_mut(), &mut err) };
        capturable.disp.unlock();
        if err.is_err() {
            Err(err)
        } else {
            Ok(Self {
                handle,
                capturable,
                img: CImage::new(),
                capture_cursor,
            })
        }
    }
}

impl Drop for RecorderX11 {
    fn drop(&mut self) {
        let mut err = CError::new();
        self.capturable.disp.lock();
        unsafe {
            stop_capture(self.handle, &mut err);
        }
        self.capturable.disp.unlock();
    }
}

impl Recorder for RecorderX11 {
    fn capture(&mut self) -> Result<PixelProvider, Box<dyn Error>> {
        let mut err = CError::new();
        self.capturable.disp.lock();
        unsafe {
            capture_screen(
                self.handle,
                &mut self.img,
                self.capture_cursor.into(),
                &mut err,
            );
        }
        self.capturable.disp.unlock();
        if err.is_err() {
            self.img.data = std::ptr::null();
            Err(err.into())
        } else {
            Ok(PixelProvider::BGR0(
                self.img.width as usize,
                self.img.height as usize,
                self.img.data(),
            ))
        }
    }
}
