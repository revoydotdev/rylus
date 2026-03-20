use std::boxed::Box;
use tracing::warn;

// Re-export core traits and types so existing consumers of rylus-capture don't break.
pub use rylus_core::{BoxCloneCapturable, Capturable, Geometry, Recorder};

#[cfg(target_os = "macos")]
pub mod core_graphics;
#[cfg(target_os = "linux")]
pub mod pipewire;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub mod remote_desktop_dbus;
pub mod testsrc;

#[cfg(target_os = "windows")]
pub mod captrs_capture;
#[cfg(target_os = "windows")]
pub mod win_ctx;
#[cfg(all(target_os = "linux", feature = "x11"))]
pub mod x11;

pub fn get_capturables(
    #[cfg(target_os = "linux")] wayland_support: bool,
    #[cfg(target_os = "linux")] capture_cursor: bool,
) -> Vec<Box<dyn Capturable>> {
    let mut capturables: Vec<Box<dyn Capturable>> = vec![];
    #[cfg(target_os = "linux")]
    {
        if wayland_support {
            use crate::pipewire::get_capturables as get_capturables_pw;
            match get_capturables_pw(capture_cursor) {
                Ok(captrs) => {
                    for c in captrs {
                        capturables.push(Box::new(c));
                    }
                }
                Err(err) => warn!(
                    "Failed to get list of capturables via dbus/pipewire: {}",
                    err
                ),
            }
        }

        #[cfg(feature = "x11")]
        {
            use crate::x11::X11Context;
            let x11ctx = X11Context::new();
            if let Some(mut x11ctx) = x11ctx {
                match x11ctx.capturables() {
                    Ok(captrs) => {
                        for c in captrs {
                            capturables.push(Box::new(c));
                        }
                    }
                    Err(err) => warn!("Failed to get list of capturables via X11: {}", err),
                }
            };
        }
    }

    #[cfg(target_os = "macos")]
    {
        use crate::core_graphics::get_displays as get_displays_cg;
        use crate::core_graphics::get_windows as get_windows_cg;
        match get_displays_cg() {
            Ok(captrs) => {
                for c in captrs {
                    capturables.push(Box::new(c));
                }
            }
            Err(err) => warn!("Failed to get list of displays via CoreGraphics: {}", err),
        }

        match get_windows_cg() {
            Ok(mut captrs) => {
                captrs.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));
                for c in captrs {
                    capturables.push(Box::new(c));
                }
            }
            Err(err) => warn!("Failed to get list of windows via CoreGraphics: {}", err),
        }
    }

    #[cfg(target_os = "windows")]
    {
        use crate::captrs_capture::CaptrsCapturable;
        use crate::win_ctx::WinCtx;
        let winctx = WinCtx::new();
        for (i, o) in winctx.get_outputs().iter().enumerate() {
            let captr = CaptrsCapturable::new(
                i as u8,
                String::from_utf16_lossy(o.DeviceName.as_ref()),
                o.DesktopCoordinates,
                winctx.get_union_rect().clone(),
            );
            capturables.push(Box::new(captr));
        }
    }

    if rylus_core::get_log_level() >= tracing::Level::DEBUG {
        for (width, height) in [
            (200, 200),
            (800, 600),
            (1080, 720),
            (1920, 1080),
            (3840, 2160),
            (15360, 2160),
        ]
        .iter()
        {
            use testsrc::PixelFormat;
            for pixel_format in [PixelFormat::BGR0, PixelFormat::RGB0, PixelFormat::RGB] {
                capturables.push(Box::new(testsrc::TestCapturable {
                    width: *width,
                    height: *height,
                    pixel_format,
                }));
            }
        }
    }

    capturables
}
