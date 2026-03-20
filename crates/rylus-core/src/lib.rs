#[macro_use]
extern crate bitflags;

pub mod config;
pub mod error;
pub mod pixel;
pub mod protocol;

use std::boxed::Box;
use std::error::Error;

use pixel::PixelProvider;

/// Message types from the web server to the GUI/controller.
pub enum Web2UiMessage {
    UInputInaccessible,
}

pub trait Recorder {
    fn capture(&mut self) -> Result<PixelProvider<'_>, Box<dyn Error>>;
}

pub trait BoxCloneCapturable {
    fn box_clone(&self) -> Box<dyn Capturable>;
}

impl<T> BoxCloneCapturable for T
where
    T: Clone + Capturable + 'static,
{
    fn box_clone(&self) -> Box<dyn Capturable> {
        Box::new(self.clone())
    }
}

/// Relative: x, y, width, height of the Capturable as floats relative to the absolute size of the
/// screen. For example x=0.5, y=0.0, width=0.5, height=1.0 means the right half of the screen.
/// VirtualScreen: offset_x, offset_y, width, height for a capturable using a virtual screen. (Windows)
pub enum Geometry {
    Relative(f64, f64, f64, f64),
    #[cfg(target_os = "windows")]
    VirtualScreen(i32, i32, u32, u32, i32, i32),
}

pub trait Capturable: Send + BoxCloneCapturable {
    /// Name of the Capturable, for example the window title, if it is a window.
    fn name(&self) -> String;

    /// Return Geometry of the Capturable.
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>>;

    /// Callback that is called right before input is simulated.
    /// Useful to focus the window on input.
    fn before_input(&mut self) -> Result<(), Box<dyn Error>>;

    /// Return a Recorder that can record the current capturable.
    fn recorder(&self, capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>>;
}

impl Clone for Box<dyn Capturable> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Utility to get the configured log level.
pub fn get_log_level() -> tracing::Level {
    #[cfg(debug_assertions)]
    let mut level = tracing::Level::DEBUG;

    #[cfg(not(debug_assertions))]
    let mut level = tracing::Level::INFO;

    if let Ok(var) = std::env::var("RYLUS_LOG_LEVEL") {
        let l: Result<tracing::Level, _> = var.parse();
        if let Ok(l) = l {
            level = l;
        }
    }
    level
}
