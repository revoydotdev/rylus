use std::io::Write;
use std::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;

struct GuiTracingWriter {
    gui_sender: mpsc::SyncSender<String>,
}

impl Write for GuiTracingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.gui_sender
            .try_send(String::from_utf8_lossy(buf).trim_start().into())
            .ok();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct GuiTracingWriterFactory {
    sender: mpsc::SyncSender<String>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for GuiTracingWriterFactory {
    type Writer = GuiTracingWriter;
    fn make_writer(&'a self) -> Self::Writer {
        Self::Writer {
            gui_sender: self.sender.clone(),
        }
    }
}

pub fn setup_logging(sender: mpsc::SyncSender<String>) {
    if std::env::var("RYLUS_LOG_JSON").is_ok() {
        let logger = tracing_subscriber::fmt()
            .json()
            .with_max_level(rylus_core::get_log_level())
            .with_writer(std::io::stdout)
            .finish()
            .with(
                tracing_subscriber::fmt::Layer::default()
                    .with_ansi(false)
                    .without_time()
                    .with_target(false)
                    .compact()
                    .with_writer(GuiTracingWriterFactory { sender }),
            );
        tracing::subscriber::set_global_default(logger).expect("Failed to setup logger!");
    } else {
        let logger = tracing_subscriber::fmt()
            .with_max_level(rylus_core::get_log_level())
            .with_writer(std::io::stderr)
            .finish()
            .with(
                tracing_subscriber::fmt::Layer::default()
                    .with_ansi(false)
                    .without_time()
                    .with_target(false)
                    .compact()
                    .with_writer(GuiTracingWriterFactory { sender }),
            );
        tracing::subscriber::set_global_default(logger).expect("Failed to setup logger!");
    }
    rylus_encode::init_ffmpeg_logger();
}

#[cfg(feature = "x11")]
mod x11_log_callbacks {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use tracing::{debug, error, info, trace, warn};

    // These #[no_mangle] functions are called from C code (log.h static inline
    // functions) which are included by X11 capture code on Linux.
    // SAFETY: The C callers always pass NUL-terminated strings from vsnprintf.
    #[no_mangle]
    extern "C" fn log_error_rust(msg: *const c_char) {
        if msg.is_null() { return; }
        let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        error!("{}", msg);
    }

    #[no_mangle]
    extern "C" fn log_debug_rust(msg: *const c_char) {
        if msg.is_null() { return; }
        let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        debug!("{}", msg);
    }

    #[no_mangle]
    extern "C" fn log_info_rust(msg: *const c_char) {
        if msg.is_null() { return; }
        let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        info!("{}", msg);
    }

    #[no_mangle]
    extern "C" fn log_trace_rust(msg: *const c_char) {
        if msg.is_null() { return; }
        let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        trace!("{}", msg);
    }

    #[no_mangle]
    extern "C" fn log_warn_rust(msg: *const c_char) {
        if msg.is_null() { return; }
        let msg = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        warn!("{}", msg);
    }
}
