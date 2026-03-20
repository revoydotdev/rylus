use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use bytes::Bytes;
use fastwebsockets::upgrade;
use handlebars::Handlebars;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rand::Rng;
use serde::Serialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use rylus_core::Web2UiMessage;
use rylus_transport::rylus_websocket_channel;

use crate::session::{RylusClientConfig, RylusClientHandler};

#[derive(Debug)]
pub enum WebStartUpMessage {
    Start,
    Error,
}

pub const INDEX_HTML: &str = std::include_str!("../../../www/templates/index.html");
pub const ACCESS_HTML: &str = std::include_str!("../../../www/static/access_code.html");
pub const STYLE_CSS: &str = std::include_str!("../../../www/static/style.css");
pub const LIB_JS: &str = std::include_str!("../../../www/static/lib.js");
pub const SETTINGS_HTML: &str = std::include_str!("../../../www/static/settings.html");

/// Maximum failed auth attempts per IP before rate limiting kicks in.
const MAX_FAILED_ATTEMPTS: u32 = 5;
/// Window for rate limiting: failed attempts older than this are forgotten.
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
/// Lockout duration after exceeding MAX_FAILED_ATTEMPTS.
const LOCKOUT_DURATION: Duration = Duration::from_secs(30);

/// Tracks failed authentication attempts per IP address.
struct RateLimiter {
    attempts: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if the IP is currently locked out.
    fn is_locked_out(&self, ip: &IpAddr) -> bool {
        let mut attempts = self.attempts.lock().expect("rate limiter mutex poisoned");
        let now = Instant::now();
        if let Some(timestamps) = attempts.get_mut(ip) {
            // Remove attempts outside the window
            timestamps.retain(|t| now.duration_since(*t) < RATE_LIMIT_WINDOW);
            if timestamps.len() as u32 >= MAX_FAILED_ATTEMPTS {
                // Check if the most recent attempt is within lockout duration
                if let Some(last) = timestamps.last() {
                    return now.duration_since(*last) < LOCKOUT_DURATION;
                }
            }
        }
        false
    }

    /// Record a failed attempt from this IP.
    fn record_failure(&self, ip: &IpAddr) {
        let mut attempts = self.attempts.lock().expect("rate limiter mutex poisoned");
        let now = Instant::now();
        let timestamps = attempts.entry(*ip).or_default();
        // Clean old entries
        timestamps.retain(|t| now.duration_since(*t) < RATE_LIMIT_WINDOW);
        timestamps.push(now);
    }
}

/// Manages authenticated sessions via random tokens stored in cookies.
struct SessionStore {
    tokens: Mutex<HashMap<String, Instant>>,
}

/// Sessions expire after 24 hours.
const SESSION_TTL: Duration = Duration::from_secs(24 * 3600);

impl SessionStore {
    fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new session, returning the token.
    fn create_session(&self) -> String {
        let mut rng = rand::thread_rng();
        let token: String = (0..32)
            .map(|_| {
                let idx = rng.gen_range(0..36);
                if idx < 10 {
                    (b'0' + idx) as char
                } else {
                    (b'a' + idx - 10) as char
                }
            })
            .collect();
        let mut tokens = self.tokens.lock().expect("session store mutex poisoned");
        tokens.insert(token.clone(), Instant::now());
        token
    }

    /// Check if a token is valid (exists and not expired).
    fn is_valid(&self, token: &str) -> bool {
        let mut tokens = self.tokens.lock().expect("session store mutex poisoned");
        if let Some(created) = tokens.get(token) {
            if created.elapsed() < SESSION_TTL {
                return true;
            }
            // Expired — remove it
            tokens.remove(token);
        }
        false
    }
}

#[derive(Serialize)]
struct IndexTemplateContext {
    access_code: Option<String>,
    uinput_enabled: bool,
    capture_cursor_enabled: bool,
    log_level: String,
    enable_custom_input_areas: bool,
}

fn response_from_str(s: &str, content_type: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(s.to_string().into())
        .expect("response builder with valid status and header should not fail")
}

fn boxed_response(
    status: StatusCode,
    content_type: &str,
    body: &str,
) -> Response<BoxBody<Bytes, Infallible>> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Full::new(Bytes::from(body.to_string())).boxed())
        .expect("response builder with valid status and header should not fail")
}

fn response_not_found() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("content-type", "text/html; charset=utf-8")
        .body("Not found!".into())
        .expect("response builder with valid status and header should not fail")
}

async fn response_from_path_or_default(
    path: Option<&PathBuf>,
    default: &str,
    content_type: &str,
) -> Response<Full<Bytes>> {
    match path {
        Some(path) => match tokio::fs::read_to_string(path).await {
            Ok(s) => response_from_str(&s, content_type),
            Err(err) => {
                warn!("Failed to load file: {}", err);
                response_from_str(default, content_type)
            }
        },
        None => response_from_str(default, content_type),
    }
}

/// Extract the session token from the Cookie header.
fn extract_session_token(req: &Request<Incoming>) -> Option<String> {
    let cookie_header = req.headers().get("cookie")?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("rylus_session=") {
            return Some(value.to_string());
        }
    }
    None
}

/// Hash an access code using argon2 with a random salt.
fn hash_access_code(code: &str) -> String {
    let salt = argon2::password_hash::SaltString::generate(&mut rand::thread_rng());
    let argon2 = Argon2::default();
    argon2
        .hash_password(code.as_bytes(), &salt)
        .expect("argon2 hashing should not fail with valid inputs")
        .to_string()
}

/// Verify an access code against an argon2 hash (constant-time comparison).
fn verify_access_code(code: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(e) => {
            warn!("Failed to parse access code hash: {e}");
            return false;
        }
    };
    Argon2::default()
        .verify_password(code.as_bytes(), &parsed)
        .is_ok()
}

async fn serve(
    addr: SocketAddr,
    mut req: Request<Incoming>,
    context: Arc<Context<'_>>,
    sender_ui: mpsc::Sender<Web2UiMessage>,
    num_clients: Arc<AtomicUsize>,
    semaphore_websocket_shutdown: Arc<tokio::sync::Semaphore>,
    notify_disconnect: Arc<tokio::sync::Notify>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, hyper::Error> {
    debug!("Got request: {:?}", req);

    // Settings page and config API don't require access_code auth
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/settings") => {
            return Ok(
                response_from_str(SETTINGS_HTML, "text/html; charset=utf-8").map(|r| r.boxed())
            );
        }
        (&Method::GET, "/api/config") => {
            return handle_get_config();
        }
        (&Method::POST, "/api/config") => {
            return handle_post_config(req).await;
        }
        _ => {}
    }

    // Handle POST /auth — access code submission
    if req.method() == Method::POST && req.uri().path() == "/auth" {
        if let Some(ref access_code_hash) = context.web_config.access_code_hash {
            // Check rate limit
            if context.rate_limiter.is_locked_out(&addr.ip()) {
                warn!(address = ?addr, "Auth attempt rejected: rate limited.");
                return Ok(boxed_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "text/html; charset=utf-8",
                    "Too many failed attempts. Please wait and try again.",
                ));
            }

            let body = match req.collect().await.map(|c| c.to_bytes()) {
                Ok(b) => b,
                Err(err) => {
                    return Ok(boxed_response(
                        StatusCode::BAD_REQUEST,
                        "text/plain",
                        &format!("Failed to read body: {err}"),
                    ));
                }
            };

            // Parse form data: access_code=xxx
            let params: HashMap<String, String> =
                url::form_urlencoded::parse(&body).into_owned().collect();

            if let Some(code) = params.get("access_code") {
                if verify_access_code(code, access_code_hash) {
                    let token = context.session_store.create_session();
                    debug!(address = ?addr, "Client authenticated via POST.");
                    return Ok(Response::builder()
                        .status(StatusCode::SEE_OTHER)
                        .header("location", "/")
                        .header(
                            "set-cookie",
                            format!(
                                "rylus_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"
                            ),
                        )
                        .body(Full::new(Bytes::from("Redirecting...")).boxed())
                        .expect("redirect response builder should not fail"));
                }
            }

            // Failed auth
            context.rate_limiter.record_failure(&addr.ip());
            warn!(address = ?addr, "Failed authentication attempt.");
            return Ok(response_from_path_or_default(
                context.web_config.custom_access_html.as_ref(),
                ACCESS_HTML,
                "text/html; charset=utf-8",
            )
            .await
            .map(|r| r.boxed()));
        }
    }

    // Determine auth status
    let authed = if context.web_config.access_code_hash.is_some() {
        // Check session cookie
        extract_session_token(&req)
            .map(|t| context.session_store.is_valid(&t))
            .unwrap_or(false)
    } else {
        true
    };

    if *req.method() != Method::GET {
        return Ok(response_not_found().map(|r| r.boxed()));
    }

    match req.uri().path() {
        "/" => {
            if !authed {
                return Ok(response_from_path_or_default(
                    context.web_config.custom_access_html.as_ref(),
                    ACCESS_HTML,
                    "text/html; charset=utf-8",
                )
                .await
                .map(|r| r.boxed()));
            }
            let config = IndexTemplateContext {
                access_code: context.web_config.access_code.clone(),
                uinput_enabled: cfg!(target_os = "linux"),
                capture_cursor_enabled: cfg!(not(target_os = "windows")),
                log_level: rylus_core::get_log_level().to_string(),
                enable_custom_input_areas: context.web_config.enable_custom_input_areas,
            };

            let html = if let Some(path) = context.web_config.custom_index_html.as_ref() {
                let mut reg = Handlebars::new();
                if let Err(err) = reg.register_template_file("index", path) {
                    warn!("Failed to register template from path: {}", err);
                    context.templates.render("index", &config)
                } else {
                    reg.render("index", &config)
                }
            } else {
                context.templates.render("index", &config)
            };

            match html {
                Ok(html) => {
                    Ok(response_from_str(&html, "text/html; charset=utf-8").map(|r| r.boxed()))
                }
                Err(err) => {
                    error!("Failed to render index template: {}", err);
                    Ok(response_not_found().map(|r| r.boxed()))
                }
            }
        }
        "/ws" => {
            if !authed {
                return Ok(Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body("unauthorized".to_string().boxed())
                    .expect("unauthorized response builder should not fail"));
            }

            let (response, fut) = match upgrade::upgrade(&mut req) {
                Ok(r) => r,
                Err(e) => {
                    warn!("WebSocket upgrade failed: {e}");
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body("WebSocket upgrade failed".to_string().boxed())
                        .expect("bad request response builder should not fail"));
                }
            };
            num_clients.fetch_add(1, Ordering::Relaxed);

            let config = context.rylus_client_config;
            tokio::spawn(async move {
                match fut.await {
                    Ok(ws) => {
                        let (sender, receiver) =
                            rylus_websocket_channel(ws, semaphore_websocket_shutdown);
                        std::thread::spawn(move || {
                            let client = RylusClientHandler::new(
                                sender,
                                receiver,
                                || {
                                    if let Err(err) =
                                        sender_ui.blocking_send(Web2UiMessage::UInputInaccessible)
                                    {
                                        warn!(
                                            "Failed to send message 'UInputInaccessible': {err}."
                                        );
                                    }
                                },
                                config,
                            );
                            client.run();
                            num_clients.fetch_sub(1, Ordering::Relaxed);
                            notify_disconnect.notify_waiters();
                        });
                    }
                    Err(err) => {
                        warn!("Error in websocket connection: {}", err);
                        num_clients.fetch_sub(1, Ordering::Relaxed);
                        notify_disconnect.notify_waiters();
                    }
                }
            });

            Ok(response.map(|r| r.boxed()))
        }
        "/style.css" => Ok(response_from_path_or_default(
            context.web_config.custom_style_css.as_ref(),
            STYLE_CSS,
            "text/css; charset=utf-8",
        )
        .await
        .map(|r| r.boxed())),
        "/lib.js" => Ok(response_from_path_or_default(
            context.web_config.custom_lib_js.as_ref(),
            LIB_JS,
            "text/javascript; charset=utf-8",
        )
        .await
        .map(|r| r.boxed())),
        _ => Ok(response_not_found().map(|r| r.boxed())),
    }
}

fn handle_get_config() -> Result<Response<BoxBody<Bytes, Infallible>>, hyper::Error> {
    use clap::Parser as _;
    let config = rylus_core::config::read_config()
        .unwrap_or_else(|| rylus_core::config::Config::parse_from::<_, &str>(["rylus"]));

    #[derive(Serialize)]
    struct SettingsResponse {
        bind_address: String,
        web_port: u16,
        access_code: Option<String>,
        #[cfg(target_os = "linux")]
        try_vaapi: bool,
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        try_nvenc: bool,
        #[cfg(target_os = "macos")]
        try_videotoolbox: bool,
        #[cfg(target_os = "windows")]
        try_mediafoundation: bool,
        #[cfg(target_os = "linux")]
        wayland_support: bool,
        platform: &'static str,
    }

    let resp = SettingsResponse {
        bind_address: config.bind_address.to_string(),
        web_port: config.web_port,
        access_code: config.access_code,
        #[cfg(target_os = "linux")]
        try_vaapi: config.try_vaapi,
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        try_nvenc: config.try_nvenc,
        #[cfg(target_os = "macos")]
        try_videotoolbox: config.try_videotoolbox,
        #[cfg(target_os = "windows")]
        try_mediafoundation: config.try_mediafoundation,
        #[cfg(target_os = "linux")]
        wayland_support: config.wayland_support,
        platform: if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "windows"
        },
    };

    let json = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".into());
    Ok(boxed_response(StatusCode::OK, "application/json", &json))
}

async fn handle_post_config(
    req: Request<Incoming>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, hyper::Error> {
    use clap::Parser as _;
    let body = req.collect().await.map(|c| c.to_bytes());
    let body = match body {
        Ok(b) => b,
        Err(err) => {
            let msg = format!("Failed to read body: {err}");
            return Ok(boxed_response(StatusCode::BAD_REQUEST, "text/plain", &msg));
        }
    };

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct SettingsUpdate {
        bind_address: Option<String>,
        web_port: Option<u16>,
        /// Tri-state: field absent = don't change, field present as null = clear, field present as string = set.
        /// We use `#[serde(default, deserialize_with)]` to distinguish "absent" (outer None) from "null" (Some(None)).
        #[serde(default, deserialize_with = "deserialize_optional_field")]
        access_code: Option<Option<String>>,
        try_vaapi: Option<bool>,
        try_nvenc: Option<bool>,
        try_videotoolbox: Option<bool>,
        try_mediafoundation: Option<bool>,
        wayland_support: Option<bool>,
    }

    fn deserialize_optional_field<'de, D>(
        deserializer: D,
    ) -> Result<Option<Option<String>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // If the field is present, deserialize it as Option<String> and wrap in Some.
        Ok(Some(Option::<String>::deserialize(deserializer)?))
    }

    use serde::Deserialize as _;

    let update: SettingsUpdate = match serde_json::from_slice(&body) {
        Ok(u) => u,
        Err(err) => {
            let msg = format!("Invalid JSON: {err}");
            return Ok(boxed_response(StatusCode::BAD_REQUEST, "text/plain", &msg));
        }
    };

    let mut config = rylus_core::config::read_config()
        .unwrap_or_else(|| rylus_core::config::Config::parse_from::<_, &str>(["rylus"]));

    if let Some(addr) = update.bind_address {
        match addr.parse() {
            Ok(a) => config.bind_address = a,
            Err(err) => {
                let msg = format!("Invalid bind address: {err}");
                return Ok(boxed_response(StatusCode::BAD_REQUEST, "text/plain", &msg));
            }
        }
    }
    if let Some(port) = update.web_port {
        config.web_port = port;
    }
    // access_code: absent = don't change, null = clear, string = set
    if let Some(code) = update.access_code {
        config.access_code = code;
    }

    #[cfg(target_os = "linux")]
    if let Some(v) = update.try_vaapi {
        config.try_vaapi = v;
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if let Some(v) = update.try_nvenc {
        config.try_nvenc = v;
    }
    #[cfg(target_os = "macos")]
    if let Some(v) = update.try_videotoolbox {
        config.try_videotoolbox = v;
    }
    #[cfg(target_os = "windows")]
    if let Some(v) = update.try_mediafoundation {
        config.try_mediafoundation = v;
    }
    #[cfg(target_os = "linux")]
    if let Some(v) = update.wayland_support {
        config.wayland_support = v;
    }

    rylus_core::config::write_config(&config);

    Ok(boxed_response(
        StatusCode::OK,
        "application/json",
        r#"{"ok":true}"#,
    ))
}

#[derive(Clone)]
pub struct WebServerConfig {
    pub bind_addr: SocketAddr,
    pub access_code: Option<String>,
    /// Pre-computed argon2 hash of the access code (None if no access code set).
    pub access_code_hash: Option<String>,
    pub custom_index_html: Option<PathBuf>,
    pub custom_access_html: Option<PathBuf>,
    pub custom_style_css: Option<PathBuf>,
    pub custom_lib_js: Option<PathBuf>,
    pub enable_custom_input_areas: bool,
}

impl WebServerConfig {
    /// Create a new WebServerConfig, hashing the access code if provided.
    pub fn new(
        bind_addr: SocketAddr,
        access_code: Option<String>,
        custom_index_html: Option<PathBuf>,
        custom_access_html: Option<PathBuf>,
        custom_style_css: Option<PathBuf>,
        custom_lib_js: Option<PathBuf>,
        enable_custom_input_areas: bool,
    ) -> Self {
        let access_code_hash = access_code.as_ref().map(|code| {
            info!("Hashing access code with argon2...");
            hash_access_code(code)
        });
        Self {
            bind_addr,
            access_code,
            access_code_hash,
            custom_index_html,
            custom_access_html,
            custom_style_css,
            custom_lib_js,
            enable_custom_input_areas,
        }
    }
}

struct Context<'a> {
    web_config: WebServerConfig,
    rylus_client_config: RylusClientConfig,
    templates: Handlebars<'a>,
    rate_limiter: RateLimiter,
    session_store: SessionStore,
}

pub fn run(
    sender_ui: tokio::sync::mpsc::Sender<Web2UiMessage>,
    sender_startup: oneshot::Sender<WebStartUpMessage>,
    notify_shutdown: Arc<tokio::sync::Notify>,
    web_server_config: WebServerConfig,
    rylus_client_config: RylusClientConfig,
) -> std::thread::JoinHandle<()> {
    let mut templates = Handlebars::new();
    templates
        .register_template_string("index", INDEX_HTML)
        .expect("built-in index template must be valid");

    let context = Context {
        web_config: web_server_config,
        rylus_client_config,
        templates,
        rate_limiter: RateLimiter::new(),
        session_store: SessionStore::new(),
    };
    std::thread::spawn(move || run_server(context, sender_ui, sender_startup, notify_shutdown))
}

#[tokio::main]
async fn run_server(
    context: Context<'static>,
    sender_ui: tokio::sync::mpsc::Sender<Web2UiMessage>,
    sender_startup: oneshot::Sender<WebStartUpMessage>,
    notify_shutdown: Arc<tokio::sync::Notify>,
) {
    let addr = context.web_config.bind_addr;

    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            error!("Failed to bind to socket: {err}.");
            if sender_startup.send(WebStartUpMessage::Error).is_err() {
                warn!("Failed to notify startup channel.");
            }
            return;
        }
    };

    if sender_startup.send(WebStartUpMessage::Start).is_err() {
        warn!("Failed to notify startup channel.");
    }

    let context = Arc::new(context);

    let broadcast_shutdown = Arc::new(tokio::sync::Notify::new());

    let num_clients = Arc::new(AtomicUsize::new(0));
    let notify_disconnect = Arc::new(tokio::sync::Notify::new());
    let semaphore_websocket_shutdown = Arc::new(tokio::sync::Semaphore::new(0));

    loop {
        let (tcp, remote_address) = tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok(conn) => conn,
                    Err(err) => {
                        warn!("Connection failed: {err}.");
                        continue;
                    }
                }
            },
            _ = notify_shutdown.notified() => {
                info!("Webserver is shutting down.");
                broadcast_shutdown.notify_waiters();
                break;
            }
        };

        debug!(address = ?remote_address, "Client connected.");

        let io = TokioIo::new(tcp);

        let sender_ui = sender_ui.clone();
        let broadcast_shutdown = broadcast_shutdown.clone();
        let context = context.clone();
        let num_clients = num_clients.clone();
        let semaphore_websocket_shutdown = semaphore_websocket_shutdown.clone();
        let notify_disconnect = notify_disconnect.clone();

        tokio::task::spawn(async move {
            let conn = http1::Builder::new().serve_connection(
                io,
                service_fn({
                    move |req| {
                        let context = context.clone();
                        let num_clients = num_clients.clone();
                        let semaphore_websocket_shutdown = semaphore_websocket_shutdown.clone();
                        let notify_disconnect = notify_disconnect.clone();
                        serve(
                            remote_address,
                            req,
                            context,
                            sender_ui.clone(),
                            num_clients,
                            semaphore_websocket_shutdown,
                            notify_disconnect,
                        )
                    }
                }),
            );

            let conn = conn.with_upgrades();

            tokio::select! {
                conn = conn => match conn {
                    Ok(_) => (),
                    Err(err) => {
                        warn!("Error polling connection ({remote_address}): {err}.")
                    }
                },
                _ = broadcast_shutdown.notified() => {
                    info!("Closing connection to: {remote_address}.");
                }
            }
        });
    }

    semaphore_websocket_shutdown.add_permits(num_clients.load(Ordering::Relaxed));

    loop {
        let remaining_clients = num_clients.load(Ordering::Relaxed);
        if remaining_clients == 0 {
            break;
        } else {
            debug!("Waiting for remaining clients ({remaining_clients}) to disconnect.");
        }
        tokio::select! {
            _ = notify_disconnect.notified() => (),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                semaphore_websocket_shutdown.add_permits(num_clients.load(Ordering::Relaxed));
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- hash_access_code / verify_access_code ----

    #[test]
    fn hash_and_verify_correct_code() {
        let code = "my_secret_code";
        let hash = hash_access_code(code);
        assert!(verify_access_code(code, &hash));
    }

    #[test]
    fn verify_wrong_code_fails() {
        let hash = hash_access_code("correct");
        assert!(!verify_access_code("wrong", &hash));
    }

    #[test]
    fn verify_empty_code() {
        let hash = hash_access_code("");
        assert!(verify_access_code("", &hash));
        assert!(!verify_access_code("notempty", &hash));
    }

    #[test]
    fn hash_produces_unique_salts() {
        let h1 = hash_access_code("same");
        let h2 = hash_access_code("same");
        // Same password, different salts -> different hashes
        assert_ne!(h1, h2);
        // But both verify
        assert!(verify_access_code("same", &h1));
        assert!(verify_access_code("same", &h2));
    }

    #[test]
    fn verify_invalid_hash_returns_false() {
        assert!(!verify_access_code("anything", "not_a_valid_hash"));
    }

    #[test]
    fn verify_with_unicode_code() {
        let code = "\u{1F680}rocket\u{2603}";
        let hash = hash_access_code(code);
        assert!(verify_access_code(code, &hash));
        assert!(!verify_access_code("rocket", &hash));
    }

    // ---- RateLimiter ----

    #[test]
    fn rate_limiter_new_ip_not_locked() {
        let rl = RateLimiter::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!rl.is_locked_out(&ip));
    }

    #[test]
    fn rate_limiter_below_threshold_not_locked() {
        let rl = RateLimiter::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
            rl.record_failure(&ip);
        }
        assert!(!rl.is_locked_out(&ip));
    }

    #[test]
    fn rate_limiter_at_threshold_locked() {
        let rl = RateLimiter::new();
        let ip: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            rl.record_failure(&ip);
        }
        assert!(rl.is_locked_out(&ip));
    }

    #[test]
    fn rate_limiter_different_ips_independent() {
        let rl = RateLimiter::new();
        let ip1: IpAddr = "10.0.0.1".parse().unwrap();
        let ip2: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            rl.record_failure(&ip1);
        }
        assert!(rl.is_locked_out(&ip1));
        assert!(!rl.is_locked_out(&ip2));
    }

    #[test]
    fn rate_limiter_ipv6() {
        let rl = RateLimiter::new();
        let ip: IpAddr = "::1".parse().unwrap();
        for _ in 0..MAX_FAILED_ATTEMPTS {
            rl.record_failure(&ip);
        }
        assert!(rl.is_locked_out(&ip));
    }

    // ---- SessionStore ----

    #[test]
    fn session_store_create_and_validate() {
        let store = SessionStore::new();
        let token = store.create_session();
        assert!(!token.is_empty());
        assert!(store.is_valid(&token));
    }

    #[test]
    fn session_store_invalid_token() {
        let store = SessionStore::new();
        assert!(!store.is_valid("nonexistent_token"));
    }

    #[test]
    fn session_store_empty_token() {
        let store = SessionStore::new();
        assert!(!store.is_valid(""));
    }

    #[test]
    fn session_store_tokens_are_unique() {
        let store = SessionStore::new();
        let t1 = store.create_session();
        let t2 = store.create_session();
        assert_ne!(t1, t2);
        assert!(store.is_valid(&t1));
        assert!(store.is_valid(&t2));
    }

    #[test]
    fn session_store_token_length() {
        let store = SessionStore::new();
        let token = store.create_session();
        assert_eq!(token.len(), 32);
    }

    #[test]
    fn session_store_token_is_alphanumeric() {
        let store = SessionStore::new();
        let token = store.create_session();
        assert!(token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    // ---- response helpers ----

    #[test]
    fn response_from_str_ok() {
        let resp = response_from_str("hello", "text/plain");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn response_not_found_status() {
        let resp = response_not_found();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn boxed_response_status_and_content_type() {
        let resp = boxed_response(StatusCode::BAD_REQUEST, "text/plain", "bad");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn boxed_response_ok_json() {
        let resp = boxed_response(StatusCode::OK, "application/json", r#"{"ok":true}"#);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ---- WebServerConfig ----

    #[test]
    fn web_server_config_no_access_code() {
        let config = WebServerConfig::new(
            "127.0.0.1:8080".parse().unwrap(),
            None,
            None,
            None,
            None,
            None,
            false,
        );
        assert!(config.access_code.is_none());
        assert!(config.access_code_hash.is_none());
    }

    #[test]
    fn web_server_config_with_access_code_hashes() {
        let config = WebServerConfig::new(
            "0.0.0.0:1701".parse().unwrap(),
            Some("testcode".into()),
            None,
            None,
            None,
            None,
            false,
        );
        assert_eq!(config.access_code.as_deref(), Some("testcode"));
        assert!(config.access_code_hash.is_some());
        // The hash should verify against the original code
        assert!(verify_access_code(
            "testcode",
            config.access_code_hash.as_ref().unwrap()
        ));
    }
}
