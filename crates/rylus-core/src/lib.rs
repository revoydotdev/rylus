#[macro_use]
extern crate bitflags;

pub mod config;
pub mod error;
pub mod pixel;
pub mod protocol;

/// Message types from the web server to the GUI/controller.
pub enum Web2UiMessage {
    UInputInaccessible,
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
