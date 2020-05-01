use std::ffi::CStr;
use std::fmt;
use std::os::raw::{c_char, c_float, c_int, c_uint, c_void};

use crate::cerror::CError;

extern "C" {
    fn get_window_info(disp: *mut c_void, windows: *mut WindowInfo, err: *mut CError) -> isize;

    fn get_root_window_info(disp: *mut c_void, root: *mut WindowInfo, err: *mut CError);

    fn get_window_geometry_relative(
        winfo: *const WindowInfo,
        x: *mut c_float,
        y: *mut c_float,
        width: *mut c_float,
        height: *mut c_float,
        err: *mut CError,
    );

    fn activate_window(window: *const WindowInfo, err: *mut CError);

    fn XOpenDisplay(name: *const c_char) -> *mut c_void;
    fn XCloseDisplay(disp: *mut c_void) -> c_int;
}

#[xwindow_info_struct(WindowInfo)]
impl WindowInfo {
    pub fn name(&self) -> String {
        unsafe { CStr::from_ptr(self.title.as_ptr()).to_string_lossy().into() }
    }

    pub fn geometry(&self) -> Result<WindowGeometry, CError> {
        let mut x: c_float = 0.0;
        let mut y: c_float = 0.0;
        let mut width: c_float = 0.0;
        let mut height: c_float = 0.0;
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        unsafe {
            get_window_geometry_relative(self, &mut x, &mut y, &mut width, &mut height, &mut err);
        }
        fltk::app::unlock();
        if err.is_err() {
            return Err(err);
        }
        Ok(WindowGeometry {
            x: x.into(),
            y: y.into(),
            width: width.into(),
            height: height.into(),
        })
    }

    pub fn activate(&self) -> Result<(), CError> {
        let mut err = CError::new();
        fltk::app::lock().unwrap();
        unsafe { activate_window(self, &mut err) }
        fltk::app::unlock();
        if err.is_err() {
            Err(err)
        } else {
            Ok(())
        }
    }
}

impl fmt::Display for WindowInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

pub struct WindowGeometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct X11Context {
    disp: *mut c_void,
}

impl X11Context {
    pub fn new() -> Option<Self> {
        let disp = unsafe { XOpenDisplay(std::ptr::null()) };
        if disp.is_null() {
            return None;
        }
        Some(Self { disp: disp })
    }

    pub fn windows(&mut self) -> Result<Vec<WindowInfo>, CError> {
        let mut err = CError::new();
        let mut window_infos = [WindowInfo::new(); 256];
        fltk::app::lock().unwrap();
        let window_count =
            unsafe { get_window_info(self.disp, window_infos.as_mut_ptr(), &mut err) as usize };
        fltk::app::unlock();
        if err.is_err() {
            return Err(err);
        }
        Ok(Vec::from(
            &window_infos[0..usize::min(window_infos.len(), window_count)],
        ))
    }

    pub fn root_window(&mut self) -> Result<WindowInfo, CError> {
        let mut err = CError::new();
        let mut root_window_info = WindowInfo::new();
        fltk::app::lock().unwrap();
        unsafe {
            get_root_window_info(self.disp, &mut root_window_info, &mut err);
        }
        fltk::app::unlock();
        if err.is_err() {
            return Err(err);
        }
        Ok(root_window_info)
    }
}

impl Drop for X11Context {
    fn drop(&mut self) {
        fltk::app::lock().unwrap();
        unsafe { XCloseDisplay(self.disp) };
        fltk::app::unlock();
    }
}
