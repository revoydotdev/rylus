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

impl Default for CError {
    fn default() -> Self {
        Self::new()
    }
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
        for (i, &b) in bytes[..len].iter().enumerate() {
            err.error_str[i] = b as c_char;
        }
        err
    }

    pub fn is_err(&self) -> bool {
        self.code != 0
    }

    pub fn code(&self) -> i32 {
        self.code
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
        // SAFETY: error_str is a fixed-size [c_char; 1024] array owned by self, so the
        // pointer is valid for 1024 bytes. We use from_bytes_until_nul to safely handle
        // the case where the C code didn't null-terminate the string.
        let bytes =
            unsafe { std::slice::from_raw_parts(self.error_str.as_ptr() as *const u8, 1024) };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cerror_is_not_err() {
        let e = CError::new();
        assert!(!e.is_err());
        assert_eq!(e.code(), 0);
    }

    #[test]
    fn new_cerror_code_is_no_error() {
        let e = CError::new();
        assert!(matches!(e.to_enum(), CErrorCode::NoError));
    }

    #[test]
    fn with_code_generic_error() {
        let e = CError::with_code(1, "generic failure");
        assert!(e.is_err());
        assert_eq!(e.code(), 1);
        assert!(matches!(e.to_enum(), CErrorCode::GenericError));
    }

    #[test]
    fn with_code_uinput_not_accessible() {
        let e = CError::with_code(101, "permission denied");
        assert!(e.is_err());
        assert_eq!(e.code(), 101);
        assert!(matches!(e.to_enum(), CErrorCode::UInputNotAccessible));
    }

    #[test]
    fn with_code_unknown_maps_to_generic() {
        let e = CError::with_code(999, "unknown");
        assert!(matches!(e.to_enum(), CErrorCode::GenericError));
    }

    #[test]
    fn display_contains_code_and_message() {
        let e = CError::with_code(42, "test message");
        let display = format!("{}", e);
        assert!(
            display.contains("42"),
            "display should contain code: {}",
            display
        );
        assert!(
            display.contains("test message"),
            "display should contain message: {}",
            display
        );
    }

    #[test]
    fn display_empty_message() {
        let e = CError::new(); // code 0, all-zeroes buffer = empty string
        let display = format!("{}", e);
        assert!(display.contains("0"));
    }

    #[test]
    fn debug_matches_display() {
        let e = CError::with_code(5, "debug test");
        let display = format!("{}", e);
        let debug = format!("{:?}", e);
        assert_eq!(display, debug);
    }

    #[test]
    fn with_code_truncates_long_message() {
        // Message longer than 1023 bytes should be truncated without panicking
        let long_msg = "x".repeat(2000);
        let e = CError::with_code(1, &long_msg);
        assert!(e.is_err());
        let display = format!("{}", e);
        // The message in display should be at most 1023 chars of 'x'
        assert!(display.contains(&"x".repeat(100)));
        // Should not contain the full 2000 chars
        assert!(!display.contains(&"x".repeat(2000)));
    }

    #[test]
    fn negative_code() {
        let e = CError::with_code(-1, "negative");
        assert!(e.is_err()); // code != 0
        assert_eq!(e.code(), -1);
        assert!(matches!(e.to_enum(), CErrorCode::GenericError));
    }

    #[test]
    fn cerror_is_error_trait() {
        let e = CError::with_code(1, "an error");
        let _: &dyn Error = &e; // Compiles = implements Error
    }
}
