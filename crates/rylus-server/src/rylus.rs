use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info, warn};

use rylus_core::config::{Config, TlsMode};
use rylus_core::Web2UiMessage;
use rylus_encode::EncoderOptions;

use crate::mdns::MdnsPublisher;
use crate::session::RylusClientConfig;
use crate::web::{WebServerConfig, WebStartUpMessage};

pub struct Rylus {
    notify_shutdown: Arc<tokio::sync::Notify>,
    web_thread: Option<std::thread::JoinHandle<()>>,
    ui_thread: Option<std::thread::JoinHandle<()>>,
    mdns_publisher: Option<MdnsPublisher>,
}

impl Rylus {
    pub fn new() -> Self {
        Self {
            notify_shutdown: Arc::new(tokio::sync::Notify::new()),
            web_thread: None,
            ui_thread: None,
            mdns_publisher: None,
        }
    }

    pub fn start(
        &mut self,
        config: &Config,
        mut on_web_message: Box<dyn FnMut(Web2UiMessage) + Send>,
    ) -> bool {
        let encoder_options = EncoderOptions {
            #[cfg(target_os = "linux")]
            try_vaapi: config.try_vaapi,
            #[cfg(not(target_os = "linux"))]
            try_vaapi: false,

            #[cfg(any(target_os = "linux", target_os = "windows"))]
            try_nvenc: config.try_nvenc,
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            try_nvenc: false,

            #[cfg(target_os = "macos")]
            try_videotoolbox: config.try_videotoolbox,
            #[cfg(not(target_os = "macos"))]
            try_videotoolbox: false,

            #[cfg(target_os = "windows")]
            try_mediafoundation: config.try_mediafoundation,
            #[cfg(not(target_os = "windows"))]
            try_mediafoundation: false,

            ..Default::default()
        };

        let tls_config = match config.resolve_tls_mode() {
            TlsMode::Disabled => {
                warn!(
                    "TLS is disabled: access codes and input events travel in cleartext. \
                     Only use this on a fully trusted network. Re-enable with --tls-mode auto."
                );
                None
            }
            TlsMode::Auto => {
                // Store cert/key under the per-user XDG state dir
                // (~/.local/state/rylus), falling back to the XDG config dir
                // (~/.config/rylus) when state_dir() is unavailable (e.g.
                // non-XDG platforms). Both are private to the current user,
                // unlike the former /tmp/rylus which was world-readable.
                let cert_dir = dirs::state_dir()
                    .or_else(dirs::config_dir)
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("rylus");
                // v2 filenames: v1 certs lacked SANs (unusable for iOS trust)
                // and had a 4096-expiry Apple rejects; regenerate rather than
                // load a stale pair.
                match crate::tls::load_or_generate_cert(
                    &cert_dir.join("cert-v2.der"),
                    &cert_dir.join("key-v2.der"),
                ) {
                    Ok(cfg) => Some(cfg),
                    Err(err) => {
                        warn!("TLS auto-setup failed, falling back to plain TCP: {err}");
                        None
                    }
                }
            }
            TlsMode::Certified => {
                let cert_path = config.tls_cert_path.as_deref().unwrap_or("");
                let key_path = config.tls_key_path.as_deref().unwrap_or("");
                if cert_path.is_empty() || key_path.is_empty() {
                    warn!("TLS mode is 'certified' but no cert/key paths provided");
                    None
                } else {
                    match crate::tls::load_or_generate_cert(
                        std::path::Path::new(cert_path),
                        std::path::Path::new(key_path),
                    ) {
                        Ok(cfg) => Some(cfg),
                        Err(err) => {
                            warn!("Failed to load TLS certificate: {err}");
                            None
                        }
                    }
                }
            }
        };

        let (sender_ui, mut receiver_ui) = tokio::sync::mpsc::channel(100);
        let (sender_startup, receiver_startup) = tokio::sync::oneshot::channel();

        let web_thread = crate::web::run(
            sender_ui,
            sender_startup,
            self.notify_shutdown.clone(),
            WebServerConfig::new(
                SocketAddr::new(config.bind_address, config.web_port),
                config.access_code.clone(),
                config.custom_index_html.clone(),
                config.custom_access_html.clone(),
                config.custom_style_css.clone(),
                config.custom_lib_js.clone(),
            ),
            RylusClientConfig {
                encoder_options,
                #[cfg(target_os = "linux")]
                wayland_support: config.wayland_support,
            },
            tls_config,
        );

        match receiver_startup.blocking_recv() {
            Ok(WebStartUpMessage::Start) => (),
            Ok(WebStartUpMessage::Error) => {
                if web_thread.join().is_err() {
                    error!("Webserver thread panicked.");
                }
                return false;
            }
            Err(err) => {
                error!("Error communicating with webserver thread: {}", err);
                if web_thread.join().is_err() {
                    error!("Webserver thread panicked.");
                }
                return false;
            }
        }
        self.web_thread = Some(web_thread);
        self.ui_thread = Some(std::thread::spawn(move || {
            while let Some(msg) = receiver_ui.blocking_recv() {
                on_web_message(msg);
            }
        }));

        if !config.no_mdns {
            let mut publisher = MdnsPublisher::new();
            if let Err(err) = publisher.register(config.web_port, None) {
                warn!("mDNS registration failed (non-fatal): {err}");
            } else {
                info!("mDNS service registered on port {}", config.web_port);
                self.mdns_publisher = Some(publisher);
            }
        }

        true
    }

    pub fn stop(&mut self) {
        if let Some(mut publisher) = self.mdns_publisher.take() {
            publisher.unregister();
        }
        self.notify_shutdown.notify_one();
        self.wait();
    }

    fn wait(&mut self) {
        if let Some(t) = self.web_thread.take() {
            if t.join().is_err() {
                error!("Web thread panicked.");
            }
        }
        if let Some(t) = self.ui_thread.take() {
            if t.join().is_err() {
                error!("UI message thread panicked.");
            }
        }
    }
}

#[cfg(feature = "gui")]
impl rylus_gui::RylusServer for Rylus {
    fn start(
        &mut self,
        config: &Config,
        on_web_message: Box<dyn FnMut(Web2UiMessage) + Send>,
    ) -> bool {
        Rylus::start(self, config, on_web_message)
    }

    fn stop(&mut self) {
        Rylus::stop(self);
    }
}

impl Drop for Rylus {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn rylus_tls_mode_resolved_from_config() {
        let config = Config::parse_from::<_, &str>(["rylus"]);
        assert_eq!(config.resolve_tls_mode(), TlsMode::Auto);
    }

    #[test]
    fn rylus_mdns_disabled_when_flag_set() {
        let config = Config::parse_from::<_, &str>(["rylus", "--no-mdns"]);
        assert!(config.no_mdns);
    }

    #[test]
    fn rylus_new_has_no_mdns_publisher() {
        let rylus = Rylus::new();
        assert!(rylus.mdns_publisher.is_none());
    }
}
