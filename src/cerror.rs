use std::error::Error;
use std::ffi::CStr;
use std::fmt;

use std::os::raw::{c_int, c_uchar};

#[repr(C)]
pub struct CError {
    code: c_int,
    error_str: [c_uchar; 1024],
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
}

impl fmt::Display for CError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CError: code: {} message: {}", self.code, unsafe {
            CStr::from_bytes_with_nul_unchecked(&self.error_str).to_string_lossy()
        })
    }
}

impl fmt::Debug for CError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for CError {}
