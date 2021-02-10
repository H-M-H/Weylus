use std::error::Error;
use std::ffi::CStr;
use std::fmt;

use std::os::raw::{c_int, c_char};

#[repr(C)]
pub struct CError {
    code: c_int,
    error_str: [c_char; 1024],
}

pub enum CErrorCode {
    NoError,
    GenericError,
    UInputNotAccessible,
}

impl CError {
    pub fn new() -> Self {
        Self {
            code: 0,
            error_str: [0; 1024],
        }
    }

    pub fn is_err(&self) -> bool {
        self.code != 0
    }

    pub fn code(&self) -> i32 {
        self.code as i32
    }

    pub fn to_enum(&self) -> CErrorCode {
        match self.code {
            0 => CErrorCode::NoError,
            101 => CErrorCode::UInputNotAccessible,
            _ => CErrorCode::GenericError,
        }
    }
}

impl fmt::Display for CError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CError: code: {} message: {}", self.code, unsafe {
            CStr::from_ptr(self.error_str.as_ptr()).to_string_lossy()
        })
    }
}

impl fmt::Debug for CError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for CError {}
