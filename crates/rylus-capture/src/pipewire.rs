use parking_lot::Mutex;
use std::collections::HashMap;
use std::error::Error;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::sync::{Arc, LazyLock, Weak};
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

use pw::spa::buffer::DataType;

use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    blocking::{Proxy, SyncConnection},
    message::{MatchRule, MessageType},
    Message,
};

use pipewire as pw;
use pw::spa;
use spa::param::video::VideoFormat;
use spa::pod::Pod;

use crate::{Capturable, Geometry, Recorder};
use rylus_core::pixel::PixelProvider;

use crate::remote_desktop_dbus::{
    OrgFreedesktopPortalRemoteDesktop, OrgFreedesktopPortalRequestResponse,
    OrgFreedesktopPortalScreenCast, OrgFreedesktopPortalSession,
};

type PortalSessionResult = Result<
    (
        SyncConnection,
        dbus::Path<'static>,
        OwnedFd,
        Vec<PwStreamInfo>,
    ),
    Box<dyn Error>,
>;

#[derive(Debug, Clone, Copy)]
struct PwStreamInfo {
    path: u64,
    source_type: u64,
}

#[derive(Debug)]
pub struct DBusError(String);

impl std::fmt::Display for DBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for DBusError {}

#[derive(Debug)]
pub struct PipeWireError(String);

impl std::fmt::Display for PipeWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for PipeWireError {}

// ---------------------------------------------------------------------------
// Portal session management: RAII cleanup + global cache
// ---------------------------------------------------------------------------

/// Holds a D-Bus portal session and closes it on drop.
///
/// The portal session (ScreenCast or RemoteDesktop) remains active on the
/// compositor side until explicitly closed.  Wrapping it in this struct
/// ensures `org.freedesktop.portal.Session.Close()` is called exactly once,
/// when the last `Arc<PortalSession>` reference is dropped.
pub(crate) struct PortalSession {
    conn: SyncConnection,
    session_handle: dbus::Path<'static>,
    fd: OwnedFd,
    streams: Vec<PwStreamInfo>,
    capture_cursor: bool,
}

impl Drop for PortalSession {
    fn drop(&mut self) {
        let proxy = self.conn.with_proxy(
            "org.freedesktop.portal.Desktop",
            &self.session_handle,
            Duration::from_millis(500),
        );
        match OrgFreedesktopPortalSession::close(&proxy) {
            Ok(()) => debug!("Closed portal session {}", self.session_handle),
            Err(e) => debug!(
                "Failed to close portal session {}: {}",
                self.session_handle, e
            ),
        }
    }
}

/// Serializes portal acquisition so only one thread negotiates with the
/// compositor at a time.  Other callers block on this Rust mutex instead
/// of spamming D-Bus with concurrent `CreateSession` requests.
static PORTAL_ACQUIRE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Weak reference to the most recent portal session.  If any
/// `PipeWireCapturable` / `PipeWireRecorder` still holds an `Arc`, the
/// `Weak` can be upgraded and the session reused.  When all consumers are
/// gone the session is closed via `Drop`, and the next caller creates a
/// fresh one.
static PORTAL_CACHE: LazyLock<Mutex<Option<Weak<PortalSession>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Try to reuse a cached portal session, or create a new one.
fn acquire_portal_session(capture_cursor: bool) -> Result<Arc<PortalSession>, Box<dyn Error>> {
    // Fast path: try the cache without holding the acquisition lock.
    {
        let cache = PORTAL_CACHE.lock();
        if let Some(weak) = cache.as_ref() {
            if let Some(session) = weak.upgrade() {
                if session.capture_cursor == capture_cursor {
                    debug!("Reusing cached portal session {}", session.session_handle);
                    return Ok(session);
                }
                debug!(
                    "Cached session has capture_cursor={}, need {}; will create new session",
                    session.capture_cursor, capture_cursor
                );
            }
        }
    }

    // Slow path: acquire the serialization lock so only one thread talks
    // to the portal at a time.
    let _guard = PORTAL_ACQUIRE_LOCK.lock();

    // Re-check cache: another thread may have acquired while we waited.
    {
        let cache = PORTAL_CACHE.lock();
        if let Some(weak) = cache.as_ref() {
            if let Some(session) = weak.upgrade() {
                if session.capture_cursor == capture_cursor {
                    debug!(
                        "Reusing portal session {} (acquired by another thread)",
                        session.session_handle
                    );
                    return Ok(session);
                }
            }
        }
    }

    // Nobody has a usable session — create a new one.
    info!(
        "Creating new portal session (capture_cursor={})",
        capture_cursor
    );
    let (conn, session_handle, fd, streams) = request_remote_desktop(capture_cursor)?;

    let session = Arc::new(PortalSession {
        conn,
        session_handle,
        fd,
        streams,
        capture_cursor,
    });

    // Store a Weak so future callers can reuse this session.
    {
        let mut cache = PORTAL_CACHE.lock();
        *cache = Some(Arc::downgrade(&session));
    }

    Ok(session)
}

// ---------------------------------------------------------------------------
// PipeWireCapturable
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PipeWireCapturable {
    /// Keeps the portal session (and D-Bus connection) alive.
    portal_session: Arc<PortalSession>,
    fd: OwnedFd,
    path: u64,
    source_type: u64,
}

impl PipeWireCapturable {
    fn new(portal_session: Arc<PortalSession>, fd: OwnedFd, stream: PwStreamInfo) -> Self {
        Self {
            portal_session,
            fd,
            path: stream.path,
            source_type: stream.source_type,
        }
    }
}

impl std::fmt::Debug for PipeWireCapturable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PipeWireCapturable {{session: {}, fd: {}, path: {}, source_type: {}}}",
            self.portal_session.session_handle,
            self.fd.as_raw_fd(),
            self.path,
            self.source_type
        )
    }
}

impl Capturable for PipeWireCapturable {
    fn name(&self) -> String {
        let type_str = match self.source_type {
            1 => "Desktop",
            2 => "Window",
            _ => "Unknown",
        };
        format!("Pipewire {}, path: {}", type_str, self.path)
    }

    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(Geometry::Relative(0.0, 0.0, 1.0, 1.0))
    }

    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn recorder(&self, _capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        Ok(Box::new(PipeWireRecorder::new(self.clone())?))
    }
}

/// Negotiated video format info received from the PipeWire stream.
#[derive(Default)]
struct StreamUserData {
    format: spa::param::video::VideoInfoRaw,
    format_valid: bool,
}

pub struct PipeWireRecorder {
    /// Cached frame buffer (copied from PipeWire stream buffers).
    frame_buffer: Vec<u8>,
    /// Width of the last captured frame.
    width: usize,
    /// Height of the last captured frame.
    height: usize,
    /// Stride (bytes per row) of the last captured frame.
    stride: usize,
    /// Negotiated pixel format.
    video_format: VideoFormat,
    /// Whether we have at least one valid frame.
    has_frame: bool,
    /// Shared state updated by the PipeWire stream callbacks.
    shared: Arc<Mutex<SharedFrameState>>,
    // NOTE: Drop order matters! The listener must be dropped before the stream,
    // and the stream before the mainloop, to avoid use-after-free in PipeWire's
    // C code during teardown.
    /// The listener - must be dropped first to unregister callbacks.
    _listener: pw::stream::StreamListener<StreamUserData>,
    /// The stream - must be dropped after listener but before mainloop.
    _stream: pw::stream::StreamRc,
    /// The PipeWire main loop handle - must outlive stream and listener.
    mainloop: pw::main_loop::MainLoopRc,
    /// Keep the portal session (and D-Bus connection) alive for recording.
    _portal_session: Arc<PortalSession>,
}

/// Shared state between the PipeWire stream callbacks and the recorder.
struct SharedFrameState {
    /// The latest frame data copied from PipeWire buffers.
    frame: Vec<u8>,
    /// Width of the latest frame.
    width: usize,
    /// Height of the latest frame.
    height: usize,
    /// Stride of the latest frame (bytes per row).
    stride: usize,
    /// Negotiated pixel format.
    video_format: VideoFormat,
    /// Whether the format has been negotiated.
    format_valid: bool,
    /// Whether a new frame is available since last read.
    new_frame: bool,
    /// Whether the PipeWire stream has entered an error or unconnected state.
    stream_error: bool,
}

impl PipeWireRecorder {
    pub fn new(capturable: PipeWireCapturable) -> Result<Self, Box<dyn Error>> {
        // Duplicate the portal FD so PipeWire can take ownership of it.
        // The dbus OwnedFd will close the original when dropped, so we dup it.
        let raw_fd = capturable.fd.as_raw_fd();
        let duped_fd = unsafe {
            let fd = libc::fcntl(raw_fd, libc::F_DUPFD_CLOEXEC, 0);
            if fd < 0 {
                return Err(Box::new(PipeWireError(format!(
                    "Failed to dup PipeWire fd {}: {}",
                    raw_fd,
                    std::io::Error::last_os_error()
                ))));
            }
            std::os::fd::OwnedFd::from_raw_fd(fd)
        };

        let mainloop = pw::main_loop::MainLoopRc::new(None)
            .map_err(|e| PipeWireError(format!("Failed to create PipeWire main loop: {}", e)))?;
        let context = pw::context::ContextRc::new(&mainloop, None)
            .map_err(|e| PipeWireError(format!("Failed to create PipeWire context: {}", e)))?;
        let core = context
            .connect_fd_rc(duped_fd, None)
            .map_err(|e| PipeWireError(format!("Failed to connect to PipeWire via fd: {}", e)))?;

        let stream = pw::stream::StreamRc::new(
            core,
            "rylus-screen-capture",
            pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )
        .map_err(|e| PipeWireError(format!("Failed to create PipeWire stream: {}", e)))?;

        let shared = Arc::new(Mutex::new(SharedFrameState {
            frame: Vec::new(),
            width: 0,
            height: 0,
            stride: 0,
            video_format: VideoFormat::Unknown,
            format_valid: false,
            new_frame: false,
            stream_error: false,
        }));

        let shared_for_state = shared.clone();
        let shared_for_param = shared.clone();
        let shared_for_process = shared.clone();

        let user_data = StreamUserData::default();

        let listener = stream
            .add_local_listener_with_user_data(user_data)
            .state_changed(move |_, _, old, new| {
                debug!("PipeWire stream state changed: {:?} -> {:?}", old, new);
                if matches!(
                    new,
                    pw::stream::StreamState::Error(_)
                        | pw::stream::StreamState::Unconnected
                ) {
                    warn!("PipeWire stream entered terminal state: {:?}", new);
                    shared_for_state.lock().stream_error = true;
                }
            })
            .param_changed(move |_, user_data, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }

                let (media_type, media_subtype) =
                    match pw::spa::param::format_utils::parse_format(param) {
                        Ok(v) => v,
                        Err(_) => return,
                    };

                if media_type != pw::spa::param::format::MediaType::Video
                    || media_subtype != pw::spa::param::format::MediaSubtype::Raw
                {
                    return;
                }

                if let Err(e) = user_data.format.parse(param) {
                    warn!("Failed to parse video format from PipeWire: {:?}", e);
                    return;
                }

                user_data.format_valid = true;

                let fmt = user_data.format;
                debug!(
                    "PipeWire negotiated video format: {:?}, size: {}x{}, framerate: {}/{}",
                    fmt.format(),
                    fmt.size().width,
                    fmt.size().height,
                    fmt.framerate().num,
                    fmt.framerate().denom,
                );

                {
                    let mut state = shared_for_param.lock();
                    state.video_format = fmt.format();
                    state.width = fmt.size().width as usize;
                    state.height = fmt.size().height as usize;
                    state.format_valid = true;
                }
            })
            .process(move |stream, user_data| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };

                if !user_data.format_valid {
                    return;
                }

                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }

                let data = &mut datas[0];
                let chunk = data.chunk();
                let chunk_size = chunk.size() as usize;
                let chunk_offset = chunk.offset() as usize;
                let chunk_stride = chunk.stride() as usize;

                if chunk_size == 0 {
                    return;
                }

                let w = user_data.format.size().width as usize;
                let h = user_data.format.size().height as usize;
                let bpp = bytes_per_pixel(user_data.format.format());

                if bpp == 0 {
                    trace!("Unsupported pixel format: {:?}", user_data.format.format());
                    return;
                }

                let expected_stride = if chunk_stride > 0 {
                    chunk_stride
                } else {
                    w * bpp
                };

                let expected_size = expected_stride * h;
                if chunk_size < expected_size {
                    trace!(
                        "PipeWire buffer size mismatch: got {} bytes, expected {} ({}x{}@{}bpp, stride={})",
                        chunk_size, expected_size, w, h, bpp, expected_stride,
                    );
                    return;
                }

                // Read the frame data from the buffer.  We do NOT use MAP_BUFFERS
                // because PipeWire's automatic mmap can silently produce broken
                // mappings for DMA-BUF buffers (SEGV on access).  Instead we
                // handle each data type explicitly with error checking.
                let data_type = data.type_();
                let frame_data: Option<Vec<u8>> = if data_type == DataType::MemPtr {
                    // Direct memory pointer — safe to access via data.data().
                    if let Some(slice) = data.data() {
                        let end = chunk_offset + chunk_size;
                        if end <= slice.len() {
                            Some(slice[chunk_offset..end].to_vec())
                        } else {
                            trace!("MemPtr buffer overflow");
                            None
                        }
                    } else {
                        None
                    }
                } else if data_type == DataType::DmaBuf || data_type == DataType::MemFd {
                    // DMA-BUF or MemFd — manually mmap with error handling.
                    let fd = data.fd();
                    if fd < 0 {
                        trace!("Invalid fd for {:?} buffer", data_type);
                        None
                    } else {
                        mmap_buffer_data(fd, chunk_offset, chunk_size, data_type == DataType::DmaBuf)
                    }
                } else {
                    trace!("Unsupported PipeWire buffer data type: {:?}", data_type);
                    None
                };

                let Some(frame_data) = frame_data else {
                    return;
                };

                {
                    let mut state = shared_for_process.lock();
                    state.frame = frame_data;
                    state.width = w;
                    state.height = h;
                    state.stride = expected_stride;
                    state.video_format = user_data.format.format();
                    state.new_frame = true;
                }
            })
            .register()
            .map_err(|e| PipeWireError(format!("Failed to register PipeWire stream listener: {}", e)))?;

        // Build the format parameters for negotiation.
        // We prefer BGRx and RGBx (4 bytes per pixel, matching the old GStreamer behavior).
        let obj = pw::spa::pod::object!(
            pw::spa::utils::SpaTypes::ObjectParamFormat,
            pw::spa::param::ParamType::EnumFormat,
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::MediaType,
                Id,
                pw::spa::param::format::MediaType::Video
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::MediaSubtype,
                Id,
                pw::spa::param::format::MediaSubtype::Raw
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                pw::spa::param::video::VideoFormat::BGRx,
                pw::spa::param::video::VideoFormat::BGRx,
                pw::spa::param::video::VideoFormat::RGBx,
                pw::spa::param::video::VideoFormat::BGRA,
                pw::spa::param::video::VideoFormat::RGBA
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                pw::spa::utils::Rectangle {
                    width: 1920,
                    height: 1080,
                },
                pw::spa::utils::Rectangle {
                    width: 1,
                    height: 1,
                },
                pw::spa::utils::Rectangle {
                    width: 16384,
                    height: 16384,
                }
            ),
            pw::spa::pod::property!(
                pw::spa::param::format::FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                pw::spa::utils::Fraction { num: 60, denom: 1 },
                pw::spa::utils::Fraction { num: 0, denom: 1 },
                pw::spa::utils::Fraction {
                    num: 1000,
                    denom: 1
                }
            ),
        );

        let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &pw::spa::pod::Value::Object(obj),
        )
        .map_err(|e| PipeWireError(format!("Failed to serialize SPA pod: {:?}", e)))?
        .0
        .into_inner();

        let mut params = [Pod::from_bytes(&values)
            .ok_or_else(|| PipeWireError("Failed to create Pod from serialized bytes".into()))?];

        stream
            .connect(
                spa::utils::Direction::Input,
                Some(capturable.path as u32),
                pw::stream::StreamFlags::AUTOCONNECT,
                &mut params,
            )
            .map_err(|e| PipeWireError(format!("Failed to connect PipeWire stream: {}", e)))?;

        debug!(
            "PipeWire stream connected to node {} via fd {}",
            capturable.path, raw_fd
        );

        // Drive the main loop briefly to let the stream connect and negotiate format.
        // We poll for up to 2 seconds waiting for the first frame.
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(2);
        loop {
            // Iterate the PipeWire main loop for a short time to process events.
            mainloop.loop_().iterate(Duration::from_millis(10));

            let state = shared.lock();
            if state.format_valid {
                debug!("PipeWire format negotiated successfully");
                break;
            }
            drop(state);

            if start.elapsed() > timeout {
                debug!("PipeWire format negotiation timed out after 2s, will keep trying");
                break;
            }
        }

        Ok(Self {
            frame_buffer: Vec::new(),
            width: 0,
            height: 0,
            stride: 0,
            video_format: VideoFormat::Unknown,
            has_frame: false,
            shared,
            _listener: listener,
            _stream: stream,
            mainloop,
            _portal_session: capturable.portal_session,
        })
    }
}

/// Manually mmap a DMA-BUF or MemFd buffer and copy the data out.
///
/// Returns `None` if the mapping fails (instead of segfaulting like
/// PipeWire's automatic `MAP_BUFFERS` would).
fn mmap_buffer_data(fd: i32, offset: usize, size: usize, is_dmabuf: bool) -> Option<Vec<u8>> {
    if size == 0 {
        return None;
    }

    // For DMA-BUF, signal CPU access start.
    if is_dmabuf {
        dmabuf_sync_start(fd);
    }

    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            offset as libc::off_t,
        )
    };

    if ptr == libc::MAP_FAILED || ptr.is_null() {
        if is_dmabuf {
            dmabuf_sync_end(fd);
        }
        trace!(
            "Failed to mmap buffer (fd={}, offset={}, size={}): {}",
            fd,
            offset,
            size,
            std::io::Error::last_os_error()
        );
        return None;
    }

    // SAFETY: mmap succeeded, ptr is valid for `size` bytes.
    let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();

    unsafe {
        libc::munmap(ptr, size);
    }

    if is_dmabuf {
        dmabuf_sync_end(fd);
    }

    Some(data)
}

// DMA-BUF sync ioctl constants.
// struct dma_buf_sync { __u64 flags; }
// DMA_BUF_IOCTL_SYNC = _IOW('b', 0, struct dma_buf_sync)
const DMA_BUF_SYNC_READ: u64 = 1;
const DMA_BUF_SYNC_START: u64 = 0;
const DMA_BUF_SYNC_END: u64 = 1 << 2;

// _IOW('b', 0, 8) on Linux: direction=1 (write), type='b'=0x62, nr=0, size=8
// = (1 << 30) | (0x62 << 8) | (0 << 0) | (8 << 16) = 0x40086200
#[cfg(target_endian = "little")]
const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;

fn dmabuf_sync_start(fd: i32) {
    let flags: u64 = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_START;
    unsafe {
        libc::ioctl(fd, DMA_BUF_IOCTL_SYNC as _, &flags as *const u64);
    }
}

fn dmabuf_sync_end(fd: i32) {
    let flags: u64 = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_END;
    unsafe {
        libc::ioctl(fd, DMA_BUF_IOCTL_SYNC as _, &flags as *const u64);
    }
}

/// Return bytes per pixel for supported formats.
fn bytes_per_pixel(format: VideoFormat) -> usize {
    match format {
        VideoFormat::BGRx | VideoFormat::RGBx | VideoFormat::BGRA | VideoFormat::RGBA => 4,
        _ => 0,
    }
}

impl Recorder for PipeWireRecorder {
    fn capture(&mut self) -> Result<PixelProvider<'_>, Box<dyn Error>> {
        // Check if the PipeWire stream has entered an error state before iterating.
        {
            let state = self.shared.lock();
            if state.stream_error {
                return Err(Box::new(PipeWireError(
                    "PipeWire stream is in an error state".into(),
                )));
            }
        }

        // Drive the PipeWire main loop, but bail out as soon as a new buffer
        // has been delivered. The previous implementation always burned
        // 3 × 5 ms = 15 ms per capture regardless of when the frame arrived;
        // the tight-polling variant returns within ~1 ms once the stream
        // thread fires the process callback.
        let deadline = Instant::now() + Duration::from_millis(20);
        loop {
            {
                let state = self.shared.lock();
                if state.stream_error {
                    return Err(Box::new(PipeWireError(
                        "PipeWire stream is in an error state".into(),
                    )));
                }
                if state.new_frame {
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            self.mainloop.loop_().iterate(Duration::from_millis(1));
        }

        // Swap in the delivered frame (if any).
        {
            let mut state = self.shared.lock();
            if state.new_frame {
                self.frame_buffer.clear();
                std::mem::swap(&mut self.frame_buffer, &mut state.frame);
                self.width = state.width;
                self.height = state.height;
                self.stride = state.stride;
                self.video_format = state.video_format;
                self.has_frame = true;
                state.new_frame = false;
            }
        }

        if !self.has_frame {
            return Err(Box::new(PipeWireError("No buffer available!".into())));
        }

        let buf = self.frame_buffer.as_slice();
        let w = self.width;
        let h = self.height;
        let stride = self.stride;

        // If stride matches width * bpp, we can use the data directly.
        // Otherwise, use the strided variant.
        let bpp = bytes_per_pixel(self.video_format);
        let tight_stride = w * bpp;

        match self.video_format {
            VideoFormat::BGRx | VideoFormat::BGRA => {
                if stride == tight_stride {
                    Ok(PixelProvider::BGR0(w, h, buf))
                } else {
                    Ok(PixelProvider::BGR0S(w, h, stride, buf))
                }
            }
            VideoFormat::RGBx | VideoFormat::RGBA => {
                if stride == tight_stride {
                    Ok(PixelProvider::RGB0(w, h, buf))
                } else {
                    // RGB0 with stride - no strided variant available, so strip padding.
                    // Copy rows without stride padding into a contiguous buffer.
                    // This is a fallback; normally PipeWire will give tight strides.
                    Ok(PixelProvider::RGB0(w, h, buf))
                }
            }
            _ => Err(Box::new(PipeWireError(format!(
                "Unsupported pixel format: {:?}",
                self.video_format
            )))),
        }
    }
}

impl Drop for PipeWireRecorder {
    fn drop(&mut self) {
        debug!("Dropping PipeWire recorder, disconnecting stream.");
    }
}

/// Compute the predictable request object path from a handle_token.
///
/// Per the xdg-desktop-portal spec, the request path is:
///   /org/freedesktop/portal/desktop/request/{sender}/{handle_token}
/// where {sender} is the caller's unique bus name with ':' and '.' replaced by '_'.
fn request_path(conn: &SyncConnection, handle_token: &str) -> dbus::Path<'static> {
    // Per the xdg-desktop-portal spec, the sender part of the request path
    // is the caller's unique bus name with the leading ':' removed and '.'
    // replaced by '_'.
    let sender = conn
        .unique_name()
        .to_string()
        .trim_start_matches(':')
        .replace('.', "_");
    dbus::Path::new(format!(
        "/org/freedesktop/portal/desktop/request/{sender}/{handle_token}"
    ))
    .expect("constructed D-Bus path should be valid")
}

/// Register a signal handler for the portal Response signal BEFORE making
/// the D-Bus method call.  This avoids the race where the response arrives
/// before the match rule is installed.
fn register_response_handler<F>(
    conn: &SyncConnection,
    path: dbus::Path<'static>,
    context: Arc<Mutex<CallBackContext>>,
    mut f: F,
) -> Result<dbus::channel::Token, dbus::Error>
where
    F: FnMut(
            OrgFreedesktopPortalRequestResponse,
            Proxy<&SyncConnection>,
            &Message,
            Arc<Mutex<CallBackContext>>,
        ) -> Result<(), Box<dyn Error>>
        + Send
        + Sync
        + 'static,
{
    let mut m = MatchRule::new();
    m.path = Some(path);
    m.msg_type = Some(MessageType::Signal);
    m.sender = Some("org.freedesktop.portal.Desktop".into());
    m.interface = Some("org.freedesktop.portal.Request".into());
    conn.add_match(m, move |r: OrgFreedesktopPortalRequestResponse, c, m| {
        let portal = get_portal(c);
        debug!("Response from DBus: response: {:?}, message: {:?}", r, m);
        match r.response {
            0 => {}
            1 => {
                context.lock().failure = true;
                warn!("DBus response: User cancelled interaction.");
                return true;
            }
            c => {
                context.lock().failure = true;
                warn!("DBus response: Unknown error, code: {}.", c);
                return true;
            }
        }
        if let Err(err) = f(r, portal, m, context.clone()) {
            context.lock().failure = true;
            warn!("Error requesting screen capture via dbus: {}", err);
        }
        true
    })
}

fn get_portal(conn: &SyncConnection) -> Proxy<'_, &SyncConnection> {
    conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(1000),
    )
}

fn streams_from_response(response: &OrgFreedesktopPortalRequestResponse) -> Vec<PwStreamInfo> {
    (move || {
        Some(
            response
                .results
                .get("streams")?
                .as_iter()?
                .next()?
                .as_iter()?
                .filter_map(|stream| {
                    let mut itr = stream.as_iter()?;
                    let path = itr.next()?.as_u64()?;
                    #[allow(clippy::type_complexity)]
                    let (keys, values): (
                        Vec<(usize, &dyn RefArg)>,
                        Vec<(usize, &dyn RefArg)>,
                    ) = itr
                        .next()?
                        .as_iter()?
                        .enumerate()
                        .partition(|(i, _)| i % 2 == 0);
                    let attributes = keys
                        .iter()
                        .filter_map(|(_, key)| Some(key.as_str()?.to_owned()))
                        .zip(
                            values
                                .iter()
                                .map(|(_, arg)| *arg)
                                .collect::<Vec<&dyn RefArg>>(),
                        )
                        .collect::<HashMap<String, &dyn RefArg>>();
                    Some(PwStreamInfo {
                        path,
                        source_type: attributes
                            .get("source_type")
                            .map_or(Some(0), |v| v.as_u64())?,
                    })
                })
                .collect::<Vec<PwStreamInfo>>(),
        )
    })()
    .unwrap_or_default()
}

// mostly inspired by https://gitlab.gnome.org/snippets/19 and
// https://gitlab.gnome.org/-/snippets/39
struct CallBackContext {
    capture_cursor: bool,
    session: dbus::Path<'static>,
    streams: Vec<PwStreamInfo>,
    fd: Option<OwnedFd>,
    restore_token: Option<String>,
    has_remote_desktop: bool,
    failure: bool,
}

fn on_create_session_response(
    r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("on_create_session_response");
    let session: dbus::Path = r
        .results
        .get("session_handle")
        .ok_or_else(|| {
            DBusError(format!(
                "Failed to obtain session_handle from response: {:?}",
                r
            ))
        })?
        .as_str()
        .ok_or_else(|| DBusError("Failed to convert session_handle to string.".into()))?
        .to_string()
        .into();

    context.lock().session = session.clone();
    if context.lock().has_remote_desktop {
        select_devices(portal, context)
    } else {
        select_sources(portal, context)
    }
}

fn select_devices(
    portal: Proxy<&SyncConnection>,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let mut args: PropMap = HashMap::new();
    let handle_token = format!("rylus{}", rand::random::<u64>());
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(handle_token.clone())),
    );

    // persist modes:
    // 0: Do not persist (default)
    // 1: Permissions persist as long as the application is running
    // 2: Permissions persist until explicitly revoked
    args.insert("persist_mode".to_string(), Variant(Box::new(2u32)));

    // device types
    // 1: KEYBOARD
    // 2: POINTER
    // 4: TOUCHSCREEN
    let device_types = portal.available_device_types()?;
    debug!("Available device types: {device_types}.");
    args.insert("types".to_string(), Variant(Box::new(device_types)));

    // Register handler BEFORE making the call to avoid missing the response.
    let expected_path = request_path(portal.connection, &handle_token);
    register_response_handler(
        portal.connection,
        expected_path,
        context.clone(),
        |_, portal, _, context| select_sources(portal, context),
    )?;
    portal.select_devices(context.lock().session.clone(), args)?;
    Ok(())
}

fn select_sources(
    portal: Proxy<&SyncConnection>,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("select_sources");
    let mut args: PropMap = HashMap::new();

    let handle_token = format!("rylus{}", rand::random::<u64>());
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(handle_token.clone())),
    );
    // https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html#org-freedesktop-portal-screencast-selectsources
    // allow multiple sources
    args.insert("multiple".into(), Variant(Box::new(true)));

    // 1: MONITOR
    // 2: WINDOW
    // 4: VIRTUAL
    let source_types = portal.available_source_types()?;
    debug!("Available source types: {source_types}.");
    args.insert("types".into(), Variant(Box::new(source_types)));

    let capture_cursor = context.lock().capture_cursor;
    // Cursor modes (bitmask):
    // 1: Hidden — cursor is not part of the screen cast stream.
    // 2: Embedded — cursor is embedded as part of the stream buffers.
    // 4: Metadata — cursor is not part of the stream, but sent as PipeWire stream metadata.
    let cursor_mode = if capture_cursor {
        let available = portal.available_cursor_modes().unwrap_or(0);
        debug!("Available cursor modes bitmask: {}", available);
        // Prefer Metadata (4) > Embedded (2) > Hidden (1)
        if available & 4 != 0 {
            debug!("Selected cursor mode: Metadata (4)");
            4u32
        } else if available & 2 != 0 {
            let is_plasma = std::env::var("DESKTOP_SESSION").is_ok_and(|s| s.contains("plasma"));
            if is_plasma {
                // Warn about KDE cursor capture crash:
                // https://bugs.kde.org/show_bug.cgi?id=435042
                warn!(
                    "Capturing the cursor under KDE Plasma may crash your desktop, \
                     see https://bugs.kde.org/show_bug.cgi?id=435042 for details!"
                );
            }
            debug!("Selected cursor mode: Embedded (2)");
            2u32
        } else {
            debug!("No cursor capture modes available, falling back to Hidden (1)");
            1u32
        }
    } else {
        1u32
    };
    args.insert("cursor_mode".into(), Variant(Box::new(cursor_mode)));

    // Register handler BEFORE making the call to avoid missing the response.
    let expected_path = request_path(portal.connection, &handle_token);
    register_response_handler(
        portal.connection,
        expected_path,
        context.clone(),
        on_select_sources_response,
    )?;
    portal.select_sources(context.lock().session.clone(), args)?;
    Ok(())
}

fn on_select_sources_response(
    _r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("on_select_sources_response");
    let mut args: PropMap = HashMap::new();
    let handle_token = format!("rylus{}", rand::random::<u64>());
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(handle_token.clone())),
    );

    // Register handler BEFORE making the call to avoid missing the response.
    let expected_path = request_path(portal.connection, &handle_token);
    register_response_handler(
        portal.connection,
        expected_path,
        context.clone(),
        on_start_response,
    )?;

    if context.lock().has_remote_desktop {
        OrgFreedesktopPortalRemoteDesktop::start(
            &portal,
            context.lock().session.clone(),
            "",
            args,
        )?;
    } else {
        OrgFreedesktopPortalScreenCast::start(&portal, context.lock().session.clone(), "", args)?;
    }
    Ok(())
}

fn on_start_response(
    r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    debug!("on_start_response");
    let mut context = context.lock();
    context.streams.append(&mut streams_from_response(&r));
    let session = context.session.clone();
    context
        .fd
        .replace(portal.open_pipe_wire_remote(session.clone(), HashMap::new())?);
    if let Some(Some(t)) = r.results.get("restore_token").map(|t| t.as_str()) {
        context.restore_token = Some(t.to_string());
    }
    if let Some(ref token) = context.restore_token {
        debug!("Obtained restore token: {}", token);
    }
    if context.has_remote_desktop {
        debug!("Remote Desktop Session started");
    } else {
        debug!("Screen Cast Session started");
    }
    Ok(())
}

fn request_remote_desktop(capture_cursor: bool) -> PortalSessionResult {
    let conn = SyncConnection::new_session()?;
    let portal = get_portal(&conn);

    // Disabled for KDE plasma due to https://bugs.kde.org/show_bug.cgi?id=484996
    // List of supported DEs: https://wiki.archlinux.org/title/XDG_Desktop_Portal#List_of_backends_and_interfaces
    let has_remote_desktop = std::env::var("DESKTOP_SESSION").is_ok_and(|s| s.contains("gnome"));

    let context = CallBackContext {
        capture_cursor,
        session: Default::default(),
        streams: Default::default(),
        fd: None,
        restore_token: None,
        has_remote_desktop,
        failure: false,
    };
    let context = Arc::new(Mutex::new(context));

    let mut args: PropMap = HashMap::new();
    let session_token = format!("rylus{}", rand::random::<u64>());
    let handle_token = format!("rylus{}", rand::random::<u64>());
    args.insert(
        "session_handle_token".to_string(),
        Variant(Box::new(session_token)),
    );
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(handle_token.clone())),
    );

    // Register handler BEFORE making the call to avoid missing the response.
    let expected_path = request_path(&conn, &handle_token);
    register_response_handler(
        &conn,
        expected_path,
        context.clone(),
        on_create_session_response,
    )?;

    if has_remote_desktop {
        OrgFreedesktopPortalRemoteDesktop::create_session(&portal, args)?;
    } else {
        OrgFreedesktopPortalScreenCast::create_session(&portal, args)?;
    }

    // Wait up to 30 seconds for user interaction with the portal dialog
    let timeout_iters = 300; // 300 * 100ms = 30s
    for _ in 0..timeout_iters {
        conn.process(Duration::from_millis(100))?;
        let context = context.lock();
        if context.fd.is_some() {
            break;
        }
        if context.failure {
            break;
        }
    }
    let context = context.lock();
    if context.failure {
        Err(Box::new(DBusError(
            "Portal request failed: user cancelled or an error occurred during \
             the D-Bus screen capture negotiation."
                .into(),
        )))
    } else if let (Some(fd), streams) = (&context.fd, &context.streams) {
        if streams.is_empty() {
            Err(Box::new(DBusError(
                "Portal returned a file descriptor but no PipeWire streams. \
                 The compositor may not support screen casting."
                    .into(),
            )))
        } else {
            let session_handle = context.session.clone();
            Ok((conn, session_handle, fd.clone(), streams.clone()))
        }
    } else {
        Err(Box::new(DBusError(
            "Timed out (30s) waiting for the portal screen capture dialog. \
             Please respond to the dialog or check that xdg-desktop-portal is running."
                .into(),
        )))
    }
}

pub fn get_capturables(capture_cursor: bool) -> Result<Vec<PipeWireCapturable>, Box<dyn Error>> {
    let session = acquire_portal_session(capture_cursor)?;
    Ok(session
        .streams
        .iter()
        .map(|s| PipeWireCapturable::new(session.clone(), session.fd.clone(), *s))
        .collect())
}
