use crate::{Capturable, Geometry, Recorder};
use captrs::Capturer;
use std::boxed::Box;
use std::error::Error;
use windows::Win32::Foundation::RECT;

#[derive(Clone)]
pub struct CaptrsCapturable {
    id: u8,
    name: String,
    screen: RECT,
    virtual_screen: RECT,
}

impl CaptrsCapturable {
    pub fn new(id: u8, name: String, screen: RECT, virtual_screen: RECT) -> CaptrsCapturable {
        CaptrsCapturable {
            id,
            name,
            screen,
            virtual_screen,
        }
    }
}

impl Capturable for CaptrsCapturable {
    fn name(&self) -> String {
        format!("Desktop {} (captrs)", self.name).into()
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recorder(&self, _capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(CaptrsRecorder::new(self.id)?))
    }
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(Geometry::VirtualScreen(
            self.screen.left - self.virtual_screen.left,
            self.screen.top - self.virtual_screen.top,
            (self.screen.right - self.screen.left) as u32,
            (self.screen.bottom - self.screen.top) as u32,
            self.screen.left,
            self.screen.top,
        ))
    }
}
#[derive(Debug)]
pub struct CaptrsError(String);

impl std::fmt::Display for CaptrsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for CaptrsError {}
pub struct CaptrsRecorder {
    capturer: Capturer,
    /// Recorder-owned byte-view of the most recently captured frame. captrs
    /// hands us a `&[Bgr8]`; we reinterpret the same bytes as `&[u8]` via a
    /// checked `from_raw_parts` (no lifetime transmute), and return a slice
    /// into this buffer to the caller. Because `capture()` takes `&mut self`,
    /// the borrow can't outlive the recorder, which is what we want.
    byte_view_ptr: *const u8,
    byte_view_len: usize,
}

// The raw pointer is only dereferenced inside `capture()` while we hold
// `&mut self`; the `Capturer` itself is not Send, so the unsafe impl just
// tracks that the pointer doesn't introduce any additional aliasing beyond
// what `Capturer` already permits.
unsafe impl Send for CaptrsRecorder {}

impl CaptrsRecorder {
    pub fn new(id: u8) -> Result<CaptrsRecorder, Box<dyn Error>> {
        Ok(CaptrsRecorder {
            capturer: Capturer::new(id.into())?,
            byte_view_ptr: std::ptr::null(),
            byte_view_len: 0,
        })
    }
}

impl Recorder for CaptrsRecorder {
    fn capture(&mut self) -> Result<rylus_core::pixel::PixelProvider<'_>, Box<dyn Error>> {
        self.capturer
            .capture_store_frame()
            .map_err(|_e| CaptrsError("Captrs failed to capture frame".into()))?;
        let (w, h) = self.capturer.geometry();
        let frame = self.capturer.get_stored_frame().ok_or_else(|| {
            Box::new(CaptrsError("No frame available after capture".into())) as Box<dyn Error>
        })?;
        // `Bgr8` is `#[repr(C)]` with four `u8` fields in captrs; reinterpret
        // as bytes without a lifetime transmute. The byte slice lives as long
        // as the `Capturer`'s internal storage — which is guarded by
        // `&mut self` here, matching the `'_` in the trait signature.
        let byte_len = std::mem::size_of_val(frame);
        self.byte_view_ptr = frame.as_ptr() as *const u8;
        self.byte_view_len = byte_len;
        // SAFETY: pointer + length come from a valid `&[Bgr8]` whose layout is
        // a contiguous run of 4 `u8` per element. Lifetime is tied to `&self`
        // via the returned reference, which the borrow checker enforces.
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(self.byte_view_ptr, self.byte_view_len) };
        Ok(rylus_core::pixel::PixelProvider::BGR0(
            w as usize,
            h as usize,
            bytes,
        ))
    }
}
