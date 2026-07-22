use parking_lot::Mutex;
use std::net::{IpAddr, SocketAddr};
use std::sync::{mpsc, Arc};

use eframe::egui;
use tracing::{info, warn};

#[cfg(not(target_os = "windows"))]
use pnet_datalink as datalink;

use rylus_core::config::{write_config, Config};
pub use rylus_core::Web2UiMessage;

use Web2UiMessage::UInputInaccessible;

// ---------------------------------------------------------------------------
// Design system colors from DESIGN.md
// ---------------------------------------------------------------------------

const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x00, 0xAA, 0xFF);
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(0x33, 0xBB, 0xFF);
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xEF, 0x44, 0x44);
const _SUCCESS_COLOR: egui::Color32 = egui::Color32::from_rgb(0x22, 0xC5, 0x5E);
const WARNING_COLOR: egui::Color32 = egui::Color32::from_rgb(0xF5, 0x9E, 0x0B);

// Dark mode
const DARK_BG: egui::Color32 = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E);
const DARK_SURFACE: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2A, 0x2A);
const DARK_SURFACE_RAISED: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
const DARK_BORDER: egui::Color32 = egui::Color32::from_rgb(0x3A, 0x3A, 0x3A);
const DARK_TEXT: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xE0, 0xE0);
const DARK_TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x88, 0x88, 0x88);
const DARK_TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x66, 0x66, 0x66);

// Light mode
const LIGHT_BG: egui::Color32 = egui::Color32::from_rgb(0xF5, 0xF5, 0xF5);
const LIGHT_SURFACE: egui::Color32 = egui::Color32::from_rgb(0xFF, 0xFF, 0xFF);
const LIGHT_BORDER: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xE0, 0xE0);
const LIGHT_TEXT: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x1A, 0x1A);
const LIGHT_TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x66, 0x66, 0x66);
const LIGHT_TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x99, 0x99, 0x99);

// ---------------------------------------------------------------------------
// Public interface (same as before)
// ---------------------------------------------------------------------------

/// Trait for controlling the Rylus server from the GUI.
/// Implemented by the server crate to break the circular dependency.
pub trait RylusServer {
    fn start(
        &mut self,
        config: &Config,
        on_web_message: Box<dyn FnMut(Web2UiMessage) + Send>,
    ) -> bool;
    fn stop(&mut self);
}

/// Server state communicated from callbacks back to the GUI.
enum ServerEvent {
    UInputInaccessible,
}

/// State of the server start/stop lifecycle.
#[derive(PartialEq, Clone)]
enum ServerState {
    Stopped,
    Starting,
    Running,
    #[allow(dead_code)]
    Error(String),
}

struct RylusApp {
    // Server
    rylus: Box<dyn RylusServer>,
    server_state: ServerState,
    server_addr: String,

    // Config fields (editable)
    access_code: String,
    bind_address: String,
    port: String,
    auto_start: bool,
    #[cfg(target_os = "linux")]
    wayland_support: bool,
    #[cfg(target_os = "linux")]
    try_vaapi: bool,
    #[cfg(target_os = "macos")]
    try_videotoolbox: bool,
    #[cfg(target_os = "windows")]
    try_mediafoundation: bool,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    try_nvenc: bool,

    // Validation errors (inline)
    bind_address_error: Option<String>,
    port_error: Option<String>,
    start_error: Option<String>,

    // Log
    log_lines: Arc<Mutex<Vec<String>>>,

    // QR code
    qr_texture: Option<egui::TextureHandle>,
    qr_url: String,

    // UInput warning (Linux)
    show_uinput_warning: bool,

    // Server events channel
    event_rx: mpsc::Receiver<ServerEvent>,
    event_tx: mpsc::Sender<ServerEvent>,

    // First-run experience
    first_run: bool,
    has_started_once: bool,

    // Collapsible section state
    connection_open: bool,
    encoding_open: bool,
    preferences_open: bool,
    log_open: bool,

    // Original config for persistence
    config: Config,

    // Access-code visibility toggle for the GUI text field.
    show_access_code: bool,
}

// Maximum number of log lines retained in the GUI log viewer before the oldest
// are dropped. Keeps memory bounded on long-running sessions.
const LOG_RING_CAPACITY: usize = 2000;

impl RylusApp {
    fn new(
        config: &Config,
        log_receiver: mpsc::Receiver<String>,
        rylus: Box<dyn RylusServer>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        let log_lines = Arc::new(Mutex::new(Vec::<String>::new()));

        // Spawn log reader thread. Cap retention at LOG_RING_CAPACITY so memory
        // doesn't grow without bound when the server is left running.
        {
            let log_lines = log_lines.clone();
            std::thread::spawn(move || {
                while let Ok(msg) = log_receiver.recv() {
                    let mut lines = log_lines.lock();
                    if lines.len() >= LOG_RING_CAPACITY {
                        let drop_n = lines.len() - LOG_RING_CAPACITY + 1;
                        lines.drain(..drop_n);
                    }
                    lines.push(msg);
                }
            });
        }

        // Detect first run: no saved config file means first run
        let first_run = rylus_core::config::read_config().is_none();

        let auto_start = config.auto_start;

        let mut app = Self {
            rylus,
            server_state: ServerState::Stopped,
            server_addr: String::new(),
            access_code: config.access_code.clone().unwrap_or_default(),
            bind_address: config.bind_address.to_string(),
            port: config.web_port.to_string(),
            auto_start,
            #[cfg(target_os = "linux")]
            wayland_support: config.wayland_support,
            #[cfg(target_os = "linux")]
            try_vaapi: config.try_vaapi,
            #[cfg(target_os = "macos")]
            try_videotoolbox: config.try_videotoolbox,
            #[cfg(target_os = "windows")]
            try_mediafoundation: config.try_mediafoundation,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            try_nvenc: config.try_nvenc,
            bind_address_error: None,
            port_error: None,
            start_error: None,
            log_lines,
            qr_texture: None,
            qr_url: String::new(),
            show_uinput_warning: false,
            event_rx,
            event_tx,
            first_run,
            has_started_once: false,
            connection_open: false,
            encoding_open: false,
            preferences_open: false,
            log_open: false,
            config: config.clone(),
            show_access_code: false,
        };

        if auto_start {
            app.start_server();
        }

        app
    }

    fn build_config(&self) -> Option<Config> {
        let bind_addr: IpAddr = match self.bind_address.parse() {
            Ok(a) => a,
            Err(_) => return None,
        };
        let web_port: u16 = match self.port.parse() {
            Ok(p) => p,
            Err(_) => return None,
        };

        let mut config = self.config.clone();
        config.access_code = if self.access_code.is_empty() {
            None
        } else {
            Some(self.access_code.clone())
        };
        config.bind_address = bind_addr;
        config.web_port = web_port;
        config.auto_start = self.auto_start;
        // gui_theme is kept from original config (no theme picker anymore)
        #[cfg(target_os = "linux")]
        {
            config.try_vaapi = self.try_vaapi;
            config.wayland_support = self.wayland_support;
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            config.try_nvenc = self.try_nvenc;
        }
        #[cfg(target_os = "macos")]
        {
            config.try_videotoolbox = self.try_videotoolbox;
        }
        #[cfg(target_os = "windows")]
        {
            config.try_mediafoundation = self.try_mediafoundation;
        }
        Some(config)
    }

    fn start_server(&mut self) {
        // Validate inputs
        self.bind_address_error = None;
        self.port_error = None;
        self.start_error = None;

        let bind_addr: IpAddr = match self.bind_address.parse() {
            Ok(a) => a,
            Err(e) => {
                self.bind_address_error = Some(format!("Invalid bind address: {}", e));
                return;
            }
        };
        let web_port: u16 = match self.port.parse() {
            Ok(p) => p,
            Err(e) => {
                self.port_error = Some(format!("Invalid port: {}", e));
                return;
            }
        };

        self.server_state = ServerState::Starting;

        let config = match self.build_config() {
            Some(c) => c,
            None => {
                self.start_error = Some("Invalid configuration".to_string());
                self.server_state = ServerState::Stopped;
                return;
            }
        };

        // Persist edits before starting so a failed bind doesn't discard the
        // user's in-flight field changes.
        self.config = config.clone();
        write_config(&config);

        let event_tx = self.event_tx.clone();
        if !self.rylus.start(
            &config,
            Box::new(move |message| match message {
                UInputInaccessible => {
                    let _ = event_tx.send(ServerEvent::UInputInaccessible);
                }
            }),
        ) {
            self.start_error = Some(
                "Failed to start server. Common causes: port already in use, \
                 bind address not reachable, or the previous server didn't shut \
                 down cleanly. See the log for details."
                    .to_string(),
            );
            self.server_state = ServerState::Stopped;
            return;
        }

        self.server_state = ServerState::Running;
        self.has_started_once = true;

        // Resolve server address
        let mut web_sock = SocketAddr::new(bind_addr, web_port);

        #[cfg(not(target_os = "windows"))]
        {
            if web_sock.ip().is_unspecified() {
                let mut ips = Vec::<IpAddr>::new();
                for iface in datalink::interfaces()
                    .iter()
                    .filter(|iface| iface.is_up() && !iface.is_loopback())
                {
                    for ipnetw in &iface.ips {
                        if (ipnetw.is_ipv4() && web_sock.ip().is_ipv4())
                            || (ipnetw.is_ipv6() && web_sock.ip().is_ipv6())
                        {
                            ips.push(ipnetw.ip());
                        }
                    }
                }
                if !ips.is_empty() {
                    web_sock.set_ip(ips[0]);
                }
                if ips.len() > 1 {
                    info!("Found more than one IP address for browsers to connect to,");
                    info!("other urls are:");
                    for ip in &ips[1..] {
                        info!("http://{}", SocketAddr::new(*ip, web_port));
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if web_sock.ip().is_unspecified() {
                self.server_addr = "http://<your ip address>".to_string();
            } else {
                self.server_addr = format!("http://{}", web_sock);
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.server_addr = format!("http://{}", web_sock);
        }

        // Build QR URL
        let mut qr_url = self.server_addr.clone();
        if let Some(code) = &config.access_code {
            qr_url.push_str("?access_code=");
            qr_url.push_str(
                &percent_encoding::utf8_percent_encode(code, percent_encoding::NON_ALPHANUMERIC)
                    .to_string(),
            );
        }
        self.qr_url = qr_url;
        self.qr_texture = None; // Force regeneration
    }

    fn stop_server(&mut self) {
        self.rylus.stop();
        self.server_state = ServerState::Stopped;
        self.server_addr.clear();
        self.qr_url.clear();
        self.qr_texture = None;
        self.start_error = None;
    }

    fn generate_qr_texture(&mut self, ctx: &egui::Context) {
        if self.qr_url.is_empty() || self.qr_texture.is_some() {
            return;
        }

        use image::Luma;
        use qrcode::QrCode;

        let code = match QrCode::new(&self.qr_url) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to generate QR code: {}", e);
                return;
            }
        };

        let img_buf = code.render::<Luma<u8>>().build();
        let dyn_image = image::DynamicImage::ImageLuma8(img_buf);
        let size = 256u32;
        let dyn_image = dyn_image.resize_exact(size, size, image::imageops::FilterType::Nearest);
        let rgba = dyn_image.to_rgba8();
        let pixels = rgba.as_flat_samples();

        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [size as usize, size as usize],
            pixels.as_slice(),
        );

        self.qr_texture =
            Some(ctx.load_texture("qr_code", color_image, egui::TextureOptions::NEAREST));
    }
}

fn apply_rylus_theme(ctx: &egui::Context) {
    let dark = ctx.style().visuals.dark_mode;

    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    if dark {
        visuals.panel_fill = DARK_BG;
        visuals.window_fill = DARK_SURFACE;
        visuals.extreme_bg_color = DARK_BG;
        visuals.faint_bg_color = DARK_SURFACE;
        visuals.override_text_color = Some(DARK_TEXT);
        visuals.widgets.noninteractive.bg_fill = DARK_SURFACE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, DARK_TEXT_SECONDARY);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, DARK_BORDER);
        visuals.widgets.inactive.bg_fill = DARK_SURFACE_RAISED;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, DARK_TEXT);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, DARK_BORDER);
        visuals.widgets.hovered.bg_fill = DARK_SURFACE_RAISED;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, ACCENT_HOVER);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.selection.bg_fill = ACCENT.linear_multiply(0.3);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.window_stroke = egui::Stroke::new(1.0_f32, DARK_BORDER);
    } else {
        visuals.panel_fill = LIGHT_BG;
        visuals.window_fill = LIGHT_SURFACE;
        visuals.extreme_bg_color = LIGHT_BG;
        visuals.faint_bg_color = LIGHT_SURFACE;
        visuals.override_text_color = Some(LIGHT_TEXT);
        visuals.widgets.noninteractive.bg_fill = LIGHT_SURFACE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, LIGHT_TEXT_SECONDARY);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LIGHT_BORDER);
        visuals.widgets.inactive.bg_fill = LIGHT_SURFACE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, LIGHT_TEXT);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, LIGHT_BORDER);
        visuals.widgets.hovered.bg_fill = LIGHT_SURFACE;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.selection.bg_fill = ACCENT.linear_multiply(0.15);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        visuals.window_stroke = egui::Stroke::new(1.0_f32, LIGHT_BORDER);
    }

    // Shared: border radius from DESIGN.md (sm: 4px)
    let rounding = egui::CornerRadius::same(4);
    visuals.widgets.noninteractive.corner_radius = rounding;
    visuals.widgets.inactive.corner_radius = rounding;
    visuals.widgets.hovered.corner_radius = rounding;
    visuals.widgets.active.corner_radius = rounding;
    visuals.widgets.open.corner_radius = rounding;

    ctx.set_visuals(visuals);

    // Spacing from DESIGN.md
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(20.0, 10.0);
    style.spacing.window_margin = egui::Margin::same(24);
    ctx.set_style(style);
}

impl eframe::App for RylusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process server events
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                ServerEvent::UInputInaccessible => {
                    self.show_uinput_warning = true;
                }
            }
        }

        apply_rylus_theme(ctx);

        // Request repaint periodically for log updates
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        // UInput warning window
        if self.show_uinput_warning {
            egui::Window::new("UInput Inaccessible")
                .collapsible(false)
                .resizable(true)
                .default_width(500.0)
                .show(ctx, |ui| {
                    ui.label(std::include_str!("strings/uinput_error.txt"));
                    ui.add_space(16.0);
                    if ui.button("Close").clicked() {
                        self.show_uinput_warning = false;
                    }
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let is_dark = ui.visuals().dark_mode;
                let text_secondary = if is_dark {
                    DARK_TEXT_SECONDARY
                } else {
                    LIGHT_TEXT_SECONDARY
                };
                let text_muted = if is_dark {
                    DARK_TEXT_MUTED
                } else {
                    LIGHT_TEXT_MUTED
                };

                ui.add_space(24.0);

                // -------------------------------------------------------
                // Hero section: Start/Stop + Connection URL + QR
                // -------------------------------------------------------
                ui.vertical_centered(|ui| {
                    // Start/Stop button (large, prominent)
                    let is_running = self.server_state == ServerState::Running;
                    let is_starting = self.server_state == ServerState::Starting;

                    let button_text = if is_starting {
                        "Starting..."
                    } else if is_running {
                        "Stop"
                    } else {
                        "Start"
                    };

                    let button_color = if is_running {
                        ERROR_COLOR
                    } else {
                        ACCENT
                    };

                    let button = egui::Button::new(
                        egui::RichText::new(button_text)
                            .size(18.0)
                            .color(egui::Color32::WHITE),
                    )
                    .fill(button_color)
                    .min_size(egui::vec2(200.0, 48.0))
                    .corner_radius(egui::CornerRadius::same(4));

                    let response = ui.add_enabled(!is_starting, button);
                    if response.clicked() {
                        if is_running {
                            self.stop_server();
                        } else {
                            self.start_server();
                        }
                    }

                    // Inline start error
                    if let Some(err) = &self.start_error {
                        ui.add_space(8.0);
                        ui.colored_label(ERROR_COLOR, err);
                    }

                    // First-run hint
                    if self.first_run && !self.has_started_once {
                        ui.add_space(8.0);
                        ui.colored_label(
                            text_secondary,
                            "Start the server, then scan the QR code on your tablet to connect.",
                        );
                    }

                    // Connection URL
                    if is_running && !self.server_addr.is_empty() {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new("Connect your tablet to:")
                                .size(13.0)
                                .color(text_secondary),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&self.server_addr)
                                    .size(15.0)
                                    .color(ACCENT)
                                    .monospace(),
                            );
                            if ui
                                .small_button("Copy")
                                .on_hover_text("Copy the connection URL to the clipboard")
                                .clicked()
                            {
                                ui.ctx().copy_text(self.server_addr.clone());
                            }
                        });

                        // QR code
                        self.generate_qr_texture(ctx);
                        if let Some(tex) = &self.qr_texture {
                            ui.add_space(16.0);
                            let size = egui::vec2(200.0, 200.0);
                            ui.image(egui::load::SizedTexture::new(tex.id(), size));
                        }
                    }
                });

                ui.add_space(24.0);
                ui.separator();

                // -------------------------------------------------------
                // Collapsible settings sections
                // -------------------------------------------------------

                // Connection Settings
                ui.add_space(8.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new("Connection").size(15.0),
                )
                .default_open(self.connection_open)
                .show(ui, |ui| {
                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new("Access Code")
                            .size(13.0)
                            .color(text_secondary),
                    );
                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.access_code)
                                .password(!self.show_access_code)
                                .hint_text("Leave blank for no access code")
                                .desired_width(256.0),
                        );
                        resp.on_hover_text(
                            "Restrict who can control your computer with an access code. \
                             Note that this does NOT do any kind of encryption and it is advised \
                             to only run Rylus inside trusted networks! Do NOT reuse any of your \
                             passwords!",
                        );
                        let toggle_label = if self.show_access_code { "Hide" } else { "Show" };
                        if ui.small_button(toggle_label).clicked() {
                            self.show_access_code = !self.show_access_code;
                        }
                    });

                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Bind Address")
                            .size(13.0)
                            .color(text_secondary),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.bind_address)
                            .desired_width(300.0),
                    );
                    if let Some(err) = &self.bind_address_error {
                        ui.colored_label(ERROR_COLOR, err);
                    }

                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Port")
                            .size(13.0)
                            .color(text_secondary),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.port)
                            .desired_width(300.0),
                    );
                    if let Some(err) = &self.port_error {
                        ui.colored_label(ERROR_COLOR, err);
                    }

                    ui.add_space(8.0);
                });

                // Encoding Settings
                egui::CollapsingHeader::new(
                    egui::RichText::new("Encoding").size(15.0),
                )
                .default_open(self.encoding_open)
                .show(ui, |ui| {
                    ui.add_space(8.0);

                    ui.label(
                        egui::RichText::new("Hardware Acceleration")
                            .size(13.0)
                            .color(text_secondary),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Video encoding can use hardware acceleration on supported systems. \
                             Quality and stability varies among hardware and drivers.",
                        )
                        .size(12.0)
                        .color(text_muted),
                    );
                    ui.add_space(4.0);

                    #[cfg(target_os = "linux")]
                    {
                        ui.checkbox(&mut self.try_vaapi, "VAAPI")
                            .on_hover_text("Try to use hardware acceleration through the Video Acceleration API.");
                    }

                    #[cfg(target_os = "macos")]
                    {
                        ui.checkbox(&mut self.try_videotoolbox, "VideoToolbox")
                            .on_hover_text("Try to use hardware acceleration through the VideoToolbox API.");
                    }

                    #[cfg(target_os = "windows")]
                    {
                        ui.checkbox(&mut self.try_mediafoundation, "MediaFoundation")
                            .on_hover_text("Try to use hardware acceleration through the MediaFoundation API.");
                    }

                    #[cfg(any(target_os = "linux", target_os = "windows"))]
                    {
                        ui.checkbox(&mut self.try_nvenc, "NVENC")
                            .on_hover_text("Try to use Nvidia's NVENC to encode the video via GPU.");
                    }

                    ui.add_space(8.0);
                });

                // Preferences
                egui::CollapsingHeader::new(
                    egui::RichText::new("Preferences").size(15.0),
                )
                .default_open(self.preferences_open)
                .show(ui, |ui| {
                    ui.add_space(8.0);

                    ui.checkbox(&mut self.auto_start, "Auto Start")
                        .on_hover_text("Start Rylus server immediately on program start.");

                    #[cfg(target_os = "linux")]
                    {
                        ui.checkbox(
                            &mut self.wayland_support,
                            "Wayland/PipeWire Support",
                        )
                        .on_hover_text(
                            "EXPERIMENTAL! This may crash your desktop! Enables screen \
                             capturing for Wayland using PipeWire and GStreamer.",
                        );
                    }

                    ui.add_space(8.0);
                });

                // First-run defaults hint
                if self.first_run && !self.has_started_once {
                    ui.add_space(4.0);
                    ui.colored_label(text_muted, "(defaults work for most setups)");
                }

                ui.add_space(8.0);
                ui.separator();

                // -------------------------------------------------------
                // Log Viewer
                // -------------------------------------------------------
                ui.add_space(8.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new("Log").size(15.0),
                )
                .default_open(self.log_open)
                .show(ui, |ui| {
                    let mut log_lines = self.log_lines.lock();

                    ui.horizontal(|ui| {
                        let count = log_lines.len();
                        ui.label(
                            egui::RichText::new(format!(
                                "{count} line{}{}",
                                if count == 1 { "" } else { "s" },
                                if count >= LOG_RING_CAPACITY {
                                    " (oldest trimmed)"
                                } else {
                                    ""
                                },
                            ))
                            .size(11.0)
                            .color(text_muted),
                        );
                        if ui
                            .add_enabled(count > 0, egui::Button::new("Clear"))
                            .on_hover_text("Clear the on-screen log buffer")
                            .clicked()
                        {
                            log_lines.clear();
                        }
                    });

                    if log_lines.is_empty() {
                        ui.colored_label(
                            text_muted,
                            "No log messages yet. Start the server to begin.",
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for line in log_lines.iter() {
                                    let color = if line.contains("ERROR") {
                                        ERROR_COLOR
                                    } else if line.contains("WARN") {
                                        WARNING_COLOR
                                    } else if line.contains("INFO") {
                                        ACCENT
                                    } else {
                                        text_muted
                                    };
                                    ui.label(
                                        egui::RichText::new(line)
                                            .monospace()
                                            .size(11.0)
                                            .color(color),
                                    );
                                }
                            });
                    }
                    ui.add_space(8.0);
                });

                ui.add_space(24.0);
            });
        });
    }
}

pub fn run(config: &Config, log_receiver: mpsc::Receiver<String>, rylus: Box<dyn RylusServer>) {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 600.0])
            .with_min_inner_size([660.0, 480.0])
            .with_title(format!("Rylus - {}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };

    let config = config.clone();
    eframe::run_native(
        &format!("Rylus - {}", env!("CARGO_PKG_VERSION")),
        native_options,
        Box::new(move |_cc| Ok(Box::new(RylusApp::new(&config, log_receiver, rylus)))),
    )
    .expect("Failed to run GUI!");
}
