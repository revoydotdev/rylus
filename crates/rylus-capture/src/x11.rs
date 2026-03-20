use crate::{Capturable, Geometry, Recorder};
use rylus_core::error::CError;
use rylus_core::pixel::PixelProvider;
use std::sync::Arc;
use std::{error::Error, fmt};

use tracing::debug;

use x11rb::connection::Connection;
use x11rb::protocol::composite::{self, ConnectionExt as _};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xinput::ConnectionExt as _;
use x11rb::protocol::xproto::{self, ConnectionExt as _, AtomEnum, Window};
use x11rb::rust_connection::RustConnection;

// SysV shared memory helpers (using libc directly)
mod sysv_shm {
    use std::ptr;

    /// Allocate a SysV shared memory segment. Returns (shm_id, address).
    pub fn alloc(size: usize) -> Result<(i32, *mut u8), String> {
        // SAFETY: IPC_PRIVATE (0) creates a new unique segment. The flags request
        // creation with rwxrwxrwx permissions.
        let shm_id = unsafe {
            libc::shmget(libc::IPC_PRIVATE, size, libc::IPC_CREAT | 0o777)
        };
        if shm_id < 0 {
            return Err(format!(
                "shmget failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // SAFETY: shm_id is a valid shared memory identifier just created by shmget.
        // Passing null lets the kernel choose the attach address.
        let addr = unsafe { libc::shmat(shm_id, ptr::null(), 0) };
        if addr == usize::MAX as *mut libc::c_void {
            // shmat failed -- clean up the segment
            unsafe { libc::shmctl(shm_id, libc::IPC_RMID, ptr::null_mut()) };
            return Err(format!(
                "shmat failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        Ok((shm_id, addr as *mut u8))
    }

    /// Detach and mark a SysV shared memory segment for removal.
    pub fn free(shm_id: i32, addr: *mut u8) {
        // SAFETY: addr was obtained from shmat for this shm_id and is detached exactly once.
        unsafe {
            libc::shmdt(addr as *const libc::c_void);
            libc::shmctl(shm_id, libc::IPC_RMID, std::ptr::null_mut());
        }
    }
}

/// No-op initialization. With x11rb (pure Rust), there is no need for XInitThreads or a
/// custom error handler -- x11rb handles protocol errors via `Result` types.
pub fn x11_init() {
    // Intentionally empty: x11rb does not require global init like Xlib.
}

// ---------------------------------------------------------------------------
// Capturable target types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum CaptureTarget {
    Window {
        win: Window,
        is_regular_window: bool,
    },
    Rect {
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    },
}

#[derive(Clone)]
pub struct X11Capturable {
    target: CaptureTarget,
    name: String,
    conn: Arc<X11Connection>,
}

// SAFETY: The x11rb RustConnection is internally safe for Send; all protocol state is
// serialized through its internal locking. Arc ensures lifetime correctness.
unsafe impl Send for X11Capturable {}

impl Capturable for X11Capturable {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        let screen = &self.conn.conn.setup().roots[self.conn.screen_num];
        let screen_width = screen.width_in_pixels as f64;
        let screen_height = screen.height_in_pixels as f64;

        match &self.target {
            CaptureTarget::Window { win, .. } => {
                let geom = self.conn.conn.get_geometry(*win)?.reply()?;
                let coords = self
                    .conn
                    .conn
                    .translate_coordinates(*win, self.conn.root, 0, 0)?
                    .reply()?;
                let x = coords.dst_x as f64 / screen_width;
                let y = coords.dst_y as f64 / screen_height;
                let w = geom.width as f64 / screen_width;
                let h = geom.height as f64 / screen_height;
                Ok(Geometry::Relative(x, y, w, h))
            }
            CaptureTarget::Rect {
                x,
                y,
                width,
                height,
            } => {
                let xf = *x as f64 / screen_width;
                let yf = *y as f64 / screen_height;
                let wf = *width as f64 / screen_width;
                let hf = *height as f64 / screen_height;
                Ok(Geometry::Relative(xf, yf, wf, hf))
            }
        }
    }

    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        if let CaptureTarget::Window {
            win,
            is_regular_window,
        } = &self.target
        {
            if !is_regular_window {
                return Ok(());
            }
            activate_window(&self.conn.conn, self.conn.root, *win)?;
        }
        Ok(())
    }

    fn recorder(&self, capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        let recorder = RecorderX11::new(self.clone(), capture_cursor)?;
        Ok(Box::new(recorder))
    }
}

impl fmt::Display for X11Capturable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

// ---------------------------------------------------------------------------
// X11 connection wrapper
// ---------------------------------------------------------------------------

struct X11Connection {
    conn: RustConnection,
    screen_num: usize,
    root: Window,
}

pub struct X11Context {
    conn: Arc<X11Connection>,
}

impl X11Context {
    pub fn new() -> Option<Self> {
        let (conn, screen_num) = RustConnection::connect(None).ok()?;
        let root = conn.setup().roots[screen_num].root;
        Some(Self {
            conn: Arc::new(X11Connection {
                conn,
                screen_num,
                root,
            }),
        })
    }

    pub fn capturables(&mut self) -> Result<Vec<X11Capturable>, CError> {
        let conn = &self.conn.conn;
        let root = self.conn.root;

        let mut capturables = Vec::new();

        // 1. Desktop (root window)
        capturables.push(X11Capturable {
            target: CaptureTarget::Window {
                win: root,
                is_regular_window: false,
            },
            name: "Desktop".into(),
            conn: self.conn.clone(),
        });

        // 2. Monitors via RandR
        match conn.randr_get_monitors(root, true) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => {
                    for mon in &reply.monitors {
                        let mon_name = match conn.get_atom_name(mon.name) {
                            Ok(cookie) => cookie
                                .reply()
                                .map(|r| String::from_utf8_lossy(&r.name).into_owned())
                                .unwrap_or_else(|_| "Unknown".into()),
                            Err(_) => "Unknown".into(),
                        };
                        capturables.push(X11Capturable {
                            target: CaptureTarget::Rect {
                                x: mon.x,
                                y: mon.y,
                                width: mon.width,
                                height: mon.height,
                            },
                            name: format!("Monitor: {}", mon_name),
                            conn: self.conn.clone(),
                        });
                    }
                }
                Err(e) => {
                    debug!("Failed to query monitor info via xrandr: {}", e);
                }
            },
            Err(e) => {
                debug!("Xrandr is unsupported on this X server: {}", e);
            }
        }

        // 3. Windows via _NET_CLIENT_LIST
        let windows = get_client_list(conn, root);
        let mut window_capturables = Vec::new();
        for win in windows {
            let title = get_window_title(conn, win);
            window_capturables.push(X11Capturable {
                target: CaptureTarget::Window {
                    win,
                    is_regular_window: true,
                },
                name: title,
                conn: self.conn.clone(),
            });
        }
        window_capturables.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        capturables.extend(window_capturables);

        Ok(capturables)
    }

    pub fn map_input_device_to_entire_screen(&mut self, device_name: &str, pen: bool) -> CError {
        match map_input_device_impl(&self.conn.conn, device_name, pen) {
            Ok(()) => CError::new(),
            Err(e) => {
                debug!("Failed to map input device to screen: {}", e);
                CError::with_code(1, &e.to_string())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Recorder (XShm-based screen capture)
// ---------------------------------------------------------------------------

pub struct RecorderX11 {
    capturable: X11Capturable,
    capture_cursor: bool,
    has_xfixes: bool,
    has_composite: bool,
    is_wayland: bool,
    shm_seg: shm::Seg,
    shm_id: i32,
    shm_addr: *mut u8,
    width: u16,
    height: u16,
    /// Track consecutive XShmGetImage failures to avoid log spam.
    last_capture_ok: bool,
}

// SAFETY: The shm_addr pointer is only accessed from RecorderX11::capture() which is &mut self,
// so there is no concurrent access. The X11 connection (via Arc) is safe for Send.
unsafe impl Send for RecorderX11 {}

impl RecorderX11 {
    pub fn new(capturable: X11Capturable, capture_cursor: bool) -> Result<Self, CError> {
        let conn = &capturable.conn.conn;

        // Check SHM extension
        if shm::query_version(conn)
            .map_err(|e| CError::with_code(1, &format!("SHM query failed: {}", e)))?
            .reply()
            .is_err()
        {
            return Err(CError::with_code(
                1,
                "XShmExtension is not available but required!",
            ));
        }

        // Check XFixes
        let has_xfixes = conn
            .xfixes_query_version(6, 0)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some();

        // Check Composite
        let has_composite = conn
            .composite_query_version(0, 4)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some();

        let is_wayland = std::env::var("XDG_SESSION_TYPE")
            .map(|v| v == "wayland")
            .unwrap_or(false);

        // Get initial geometry
        let (_, _, width, height) = get_target_geometry(conn, capturable.conn.root, &capturable.target)?;

        // If this is a regular window and composite is available, redirect it
        if has_composite {
            if let CaptureTarget::Window { win, is_regular_window: true } = &capturable.target {
                let _ = conn.composite_redirect_window(*win, composite::Redirect::AUTOMATIC);
                let _ = conn.flush();
            }
        }

        // Allocate shared memory segment
        let bytes_per_pixel = 4u32; // ZPixmap with depth >= 24 is always 4 bytes/pixel
        let seg_size = width as u32 * height as u32 * bytes_per_pixel;

        let (shm_id, shm_addr) = sysv_shm::alloc(seg_size as usize)
            .map_err(|e| CError::with_code(1, &e))?;

        let shm_seg = conn.generate_id().map_err(|e| CError::with_code(1, &e.to_string()))?;
        conn.shm_attach(shm_seg, shm_id as u32, false)
            .map_err(|e| CError::with_code(1, &format!("ShmAttach failed: {}", e)))?;
        let _ = conn.flush();

        Ok(Self {
            capturable,
            capture_cursor,
            has_xfixes,
            has_composite,
            is_wayland,
            shm_seg,
            shm_id,
            shm_addr: shm_addr as *mut u8,
            width,
            height,
            last_capture_ok: true,
        })
    }

    /// Reallocate the shared memory segment when the target has been resized.
    fn reallocate_shm(&mut self, new_width: u16, new_height: u16) -> Result<(), CError> {
        let conn = &self.capturable.conn.conn;

        // Detach old segment
        let _ = conn.shm_detach(self.shm_seg);
        let _ = conn.flush();
        sysv_shm::free(self.shm_id, self.shm_addr);

        let bytes_per_pixel = 4u32;
        let seg_size = new_width as u32 * new_height as u32 * bytes_per_pixel;

        let (shm_id, shm_addr) = sysv_shm::alloc(seg_size as usize)
            .map_err(|e| CError::with_code(1, &e))?;

        let shm_seg = conn
            .generate_id()
            .map_err(|e| CError::with_code(1, &e.to_string()))?;
        conn.shm_attach(shm_seg, shm_id as u32, false)
            .map_err(|e| CError::with_code(1, &format!("ShmAttach failed: {}", e)))?;
        let _ = conn.flush();

        self.shm_seg = shm_seg;
        self.shm_id = shm_id;
        self.shm_addr = shm_addr;
        self.width = new_width;
        self.height = new_height;

        Ok(())
    }
}

impl Drop for RecorderX11 {
    fn drop(&mut self) {
        let conn = &self.capturable.conn.conn;

        // Unredirect if we redirected
        if self.has_composite {
            if let CaptureTarget::Window { win, is_regular_window: true } = &self.capturable.target {
                let _ = conn.composite_unredirect_window(*win, composite::Redirect::AUTOMATIC);
            }
        }

        let _ = conn.shm_detach(self.shm_seg);
        let _ = conn.flush();
        sysv_shm::free(self.shm_id, self.shm_addr);
    }
}

impl Recorder for RecorderX11 {
    fn capture(&mut self) -> Result<PixelProvider<'_>, Box<dyn Error>> {
        let root = self.capturable.conn.root;
        let screen_num = self.capturable.conn.screen_num;

        let (x, y, width, height) =
            get_target_geometry(&self.capturable.conn.conn, root, &self.capturable.target)?;

        // Resize shm if the target changed size
        if width != self.width || height != self.height {
            self.reallocate_shm(width, height)?;
        }

        let conn = &self.capturable.conn.conn;
        let screen = &conn.setup().roots[screen_num];

        let capture_ok = match &self.capturable.target {
            CaptureTarget::Window {
                win,
                is_regular_window,
            } => {
                let is_offscreen = *is_regular_window
                    && (x < 0
                        || y < 0
                        || x + width as i32 > screen.width_in_pixels as i32
                        || y + height as i32 > screen.height_in_pixels as i32);

                if !self.is_wayland && !is_offscreen {
                    // Check if this is the active window
                    let is_active = get_active_window(conn, root)
                        .map(|aw| aw == *win)
                        .unwrap_or(false);

                    if is_active {
                        // Capture from root at the window's position to include menus
                        shm_get_image(conn, root, x as i16, y as i16, self.width, self.height, self.shm_seg)
                    } else if is_offscreen {
                        capture_offscreen(conn, *win, self.has_composite, self.width, self.height, self.shm_seg)
                    } else {
                        shm_get_image(conn, *win, 0, 0, self.width, self.height, self.shm_seg)
                    }
                } else if is_offscreen {
                    capture_offscreen(conn, *win, self.has_composite, self.width, self.height, self.shm_seg)
                } else {
                    shm_get_image(conn, *win, 0, 0, self.width, self.height, self.shm_seg)
                }
            }
            CaptureTarget::Rect { .. } => {
                shm_get_image(conn, root, x as i16, y as i16, self.width, self.height, self.shm_seg)
            }
        };

        let last_ok = self.last_capture_ok;
        self.last_capture_ok = capture_ok;

        if !capture_ok {
            let code = if last_ok { 1 } else { 2 };
            return Err(Box::new(CError::with_code(code, "XShmGetImage failed!")));
        }

        // Composite cursor if requested
        if self.capture_cursor && self.has_xfixes {
            composite_cursor(conn, self.shm_addr, x, y, self.width, self.height);
        }

        let size = self.width as usize * self.height as usize * 4;
        // SAFETY: shm_addr points to shared memory of at least `size` bytes, allocated in
        // new() or reallocate_shm(). The data is valid until the next capture() call or drop,
        // and the borrow lifetime of PixelProvider is tied to &mut self.
        let data = unsafe { std::slice::from_raw_parts(self.shm_addr, size) };
        Ok(PixelProvider::BGR0(
            self.width as usize,
            self.height as usize,
            data,
        ))
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn get_target_geometry(
    conn: &RustConnection,
    root: Window,
    target: &CaptureTarget,
) -> Result<(i32, i32, u16, u16), CError> {
    match target {
        CaptureTarget::Window { win, .. } => {
            let geom = conn
                .get_geometry(*win)
                .map_err(|e| CError::with_code(1, &format!("Failed to get window geometry: {}", e)))?
                .reply()
                .map_err(|e| CError::with_code(1, &format!("Failed to get window geometry: {}", e)))?;
            let coords = conn
                .translate_coordinates(*win, root, 0, 0)
                .map_err(|e| CError::with_code(1, &format!("translate_coordinates failed: {}", e)))?
                .reply()
                .map_err(|e| CError::with_code(1, &format!("translate_coordinates failed: {}", e)))?;
            Ok((
                coords.dst_x as i32,
                coords.dst_y as i32,
                geom.width,
                geom.height,
            ))
        }
        CaptureTarget::Rect {
            x,
            y,
            width,
            height,
        } => Ok((*x as i32, *y as i32, *width, *height)),
    }
}

fn shm_get_image(
    conn: &RustConnection,
    drawable: Window,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    shm_seg: shm::Seg,
) -> bool {
    conn.shm_get_image(
        drawable,
        x,
        y,
        width,
        height,
        0x00ffffff,
        xproto::ImageFormat::Z_PIXMAP.into(),
        shm_seg,
        0,
    )
    .ok()
    .and_then(|cookie| cookie.reply().ok())
    .is_some()
}

fn capture_offscreen(
    conn: &RustConnection,
    win: Window,
    has_composite: bool,
    width: u16,
    height: u16,
    shm_seg: shm::Seg,
) -> bool {
    if has_composite {
        if let Ok(pixmap_id) = conn.generate_id() {
            if conn.composite_name_window_pixmap(win, pixmap_id).is_ok() {
                let _ = conn.flush();
                let result = shm_get_image(conn, pixmap_id, 0, 0, width, height, shm_seg);
                let _ = conn.free_pixmap(pixmap_id);
                let _ = conn.flush();
                return result;
            }
        }
    }
    false
}

fn get_active_window(conn: &RustConnection, root: Window) -> Option<Window> {
    let atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let prop = conn
        .get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    if prop.value_len == 0 {
        return None;
    }
    let values: Vec<u32> = prop.value32()?.collect();
    values.into_iter().next()
}

fn get_client_list(conn: &RustConnection, root: Window) -> Vec<Window> {
    // Try _NET_CLIENT_LIST first, then _WIN_CLIENT_LIST
    if let Some(list) = get_window_list_property(conn, root, b"_NET_CLIENT_LIST", AtomEnum::WINDOW)
    {
        return list;
    }
    if let Some(list) =
        get_window_list_property(conn, root, b"_WIN_CLIENT_LIST", AtomEnum::CARDINAL)
    {
        return list;
    }
    debug!("Cannot get client list properties (_NET_CLIENT_LIST or _WIN_CLIENT_LIST)");
    Vec::new()
}

fn get_window_list_property(
    conn: &RustConnection,
    root: Window,
    prop_name: &[u8],
    prop_type: AtomEnum,
) -> Option<Vec<Window>> {
    let atom = conn.intern_atom(false, prop_name).ok()?.reply().ok()?.atom;
    let prop = conn
        .get_property(false, root, atom, prop_type, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    let values: Vec<u32> = prop.value32()?.collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn get_window_title(conn: &RustConnection, win: Window) -> String {
    // Try _NET_WM_NAME (UTF-8) first
    if let Some(title) = get_utf8_property(conn, win, b"_NET_WM_NAME") {
        if !title.is_empty() {
            return title;
        }
    }
    // Fall back to WM_NAME
    if let Some(title) = get_string_property(conn, win, b"WM_NAME") {
        if !title.is_empty() {
            return title;
        }
    }
    format!("UNKNOWN 0x{:x}", win)
}

fn get_utf8_property(conn: &RustConnection, win: Window, prop_name: &[u8]) -> Option<String> {
    let atom = conn.intern_atom(false, prop_name).ok()?.reply().ok()?.atom;
    let utf8_atom = conn
        .intern_atom(false, b"UTF8_STRING")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let prop = conn
        .get_property(false, win, atom, utf8_atom, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    if prop.value_len == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&prop.value).into_owned())
}

fn get_string_property(conn: &RustConnection, win: Window, prop_name: &[u8]) -> Option<String> {
    let atom = conn.intern_atom(false, prop_name).ok()?.reply().ok()?.atom;
    let prop = conn
        .get_property(false, win, atom, AtomEnum::STRING, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    if prop.value_len == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&prop.value).into_owned())
}

fn activate_window(
    conn: &RustConnection,
    root: Window,
    win: Window,
) -> Result<(), Box<dyn Error>> {
    // Check if already active
    if let Some(active) = get_active_window(conn, root) {
        if active == win {
            return Ok(());
        }
    }

    // Switch to the window's desktop
    if let Some(desktop) = get_cardinal_property(conn, win, b"_NET_WM_DESKTOP")
        .or_else(|| get_cardinal_property(conn, win, b"_WIN_WORKSPACE"))
    {
        send_client_message(conn, root, root, b"_NET_CURRENT_DESKTOP", [desktop, 0, 0, 0, 0])?;
    }

    // Activate window
    send_client_message(conn, root, win, b"_NET_ACTIVE_WINDOW", [0, 0, 0, 0, 0])?;

    // Raise it
    conn.configure_window(
        win,
        &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
    )?;
    conn.map_window(win)?;
    conn.flush()?;

    Ok(())
}

fn get_cardinal_property(conn: &RustConnection, win: Window, prop_name: &[u8]) -> Option<u32> {
    let atom = conn.intern_atom(false, prop_name).ok()?.reply().ok()?.atom;
    let prop = conn
        .get_property(false, win, atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let values: Vec<u32> = prop.value32()?.collect();
    values.into_iter().next()
}

fn send_client_message(
    conn: &RustConnection,
    root: Window,
    target: Window,
    msg_name: &[u8],
    data: [u32; 5],
) -> Result<(), Box<dyn Error>> {
    let msg_atom = conn.intern_atom(false, msg_name)?.reply()?.atom;
    let event = xproto::ClientMessageEvent::new(
        32,
        target,
        msg_atom,
        xproto::ClientMessageData::from(data),
    );
    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.flush()?;
    Ok(())
}

fn composite_cursor(
    conn: &RustConnection,
    shm_addr: *mut u8,
    cap_x: i32,
    cap_y: i32,
    cap_width: u16,
    cap_height: u16,
) {
    let cursor_img = match conn.xfixes_get_cursor_image() {
        Ok(cookie) => match cookie.reply() {
            Ok(img) => img,
            Err(_) => return,
        },
        Err(_) => return,
    };

    let cursor_x = cursor_img.x as i32 - cursor_img.xhot as i32 - cap_x;
    let cursor_y = cursor_img.y as i32 - cursor_img.yhot as i32 - cap_y;
    let cw = cursor_img.width as i32;
    let ch = cursor_img.height as i32;
    let w = cap_width as i32;
    let h = cap_height as i32;

    let i0 = 0i32.max(-cursor_x).min(w - cursor_x).min(cw);
    let i1 = (cw).min(w - cursor_x).max(0);
    let j0 = 0i32.max(-cursor_y).min(h - cursor_y).min(ch);
    let j1 = (ch).min(h - cursor_y).max(0);

    if i0 >= i1 || j0 >= j1 {
        return;
    }

    // SAFETY: shm_addr points to shared memory of cap_width * cap_height * 4 bytes.
    // We only write within bounds guaranteed by the clamping above.
    let data =
        unsafe { std::slice::from_raw_parts_mut(shm_addr as *mut u32, (w * h) as usize) };

    let cursor_pixels = &cursor_img.cursor_image;

    for j in j0..j1 {
        for i in i0..i1 {
            let c_pixel = cursor_pixels[(j * cw + i) as usize];
            let a = (c_pixel >> 24) & 0xff;
            if a > 0 {
                let dst_idx = ((j + cursor_y) * w + (i + cursor_x)) as usize;
                let d_pixel = data[dst_idx];

                // Cursor image colors are premultiplied with alpha
                let c_r = (c_pixel >> 16) & 0xff;
                let c_g = (c_pixel >> 8) & 0xff;
                let c_b = c_pixel & 0xff;
                let d_r = (d_pixel >> 16) & 0xff;
                let d_g = (d_pixel >> 8) & 0xff;
                let d_b = d_pixel & 0xff;

                let f_r = c_r + d_r * (255 - a) / 255;
                let f_g = c_g + d_g * (255 - a) / 255;
                let f_b = c_b + d_b * (255 - a) / 255;

                data[dst_idx] = (f_r << 16) | (f_g << 8) | f_b;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// XInput2 device mapping
// ---------------------------------------------------------------------------

fn map_input_device_impl(
    conn: &RustConnection,
    device_name: &str,
    pen: bool,
) -> Result<(), Box<dyn Error>> {
    let pen_prefix = if pen {
        Some(format!("{} Pen", device_name))
    } else {
        None
    };

    // List input devices via XInput
    let devices = conn.xinput_list_input_devices()?.reply()?;

    let mut device_id: Option<u8> = None;
    for (dev, name_str_info) in devices.devices.iter().zip(devices.names.iter()) {
        // The name is stored as bytes in the Str struct
        let name_str = String::from_utf8_lossy(&name_str_info.name).into_owned();

        let matches = if let Some(prefix) = &pen_prefix {
            name_str.starts_with(prefix)
        } else {
            name_str == device_name
        };

        if matches {
            device_id = Some(dev.device_id);
            break;
        }
    }

    let device_id = device_id.ok_or_else(|| {
        format!("Device with name: {} not found!", device_name)
    })?;

    let float_atom = conn.intern_atom(false, b"FLOAT")?.reply()?.atom;
    let matrix_atom = conn
        .intern_atom(false, b"Coordinate Transformation Matrix")?
        .reply()?
        .atom;

    if float_atom == x11rb::NONE || matrix_atom == x11rb::NONE {
        return Err("Float or Coordinate Transformation Matrix atom not found".into());
    }

    // Set the identity matrix
    let identity: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let items: Vec<u32> = identity
        .iter()
        .map(|f| u32::from_ne_bytes(f.to_ne_bytes()))
        .collect();

    use x11rb::protocol::xinput::ChangeDevicePropertyAux;
    let data = ChangeDevicePropertyAux::Data32(items);

    conn.xinput_change_device_property(
        matrix_atom,
        float_atom,
        device_id,
        xproto::PropMode::REPLACE,
        9,
        &data,
    )?;
    conn.flush()?;

    Ok(())
}
