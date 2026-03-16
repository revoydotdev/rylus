use std::error::Error;
use std::ffi::CStr;
use std::fmt;

use std::os::raw::{c_char, c_int};

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

    pub fn with_code(code: i32, msg: &str) -> Self {
        let mut err = Self::new();
        err.code = code as c_int;
        let bytes = msg.as_bytes();
        let len = bytes.len().min(1023);
        for i in 0..len {
            err.error_str[i] = bytes[i] as c_char;
        }
        err
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
        // Defensively read the error string, ensuring we don't read past the buffer
        // even if the C code didn't null-terminate it.
        let bytes = unsafe {
            std::slice::from_raw_parts(self.error_str.as_ptr() as *const u8, 1024)
        };
        let msg = match CStr::from_bytes_until_nul(bytes) {
            Ok(s) => s.to_string_lossy(),
            Err(_) => std::borrow::Cow::Borrowed("<invalid error>"),
        };
        write!(f, "CError: code: {} message: {}", self.code, msg)
    }
}

impl fmt::Debug for CError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for CError {}
