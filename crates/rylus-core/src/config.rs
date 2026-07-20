use std::net::IpAddr;
use std::{fs, path::PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(clap::ValueEnum, Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ThemeType {
    Aero,
    AquaClassic,
    Blue,
    Classic,
    Dark,
    #[default]
    Greybird,
    HighContrast,
    Metro,
}

const THEME_LIST: [ThemeType; 8] = [
    ThemeType::Aero,
    ThemeType::AquaClassic,
    ThemeType::Blue,
    ThemeType::Classic,
    ThemeType::Dark,
    ThemeType::Greybird,
    ThemeType::HighContrast,
    ThemeType::Metro,
];

impl ThemeType {
    pub fn name(&self) -> String {
        format!("{self:?}")
    }

    pub fn to_index(&self) -> i32 {
        THEME_LIST
            .iter()
            .position(|th| th == self)
            .expect("Theme variant missing from THEME_LIST") as i32
    }

    pub fn from_index(i: i32) -> Self {
        let i = i.clamp(0, THEME_LIST.len() as i32 - 1) as usize;
        THEME_LIST[i]
    }

    pub fn themes() -> &'static [ThemeType] {
        &THEME_LIST
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    #[default]
    Disabled,
    Auto,
    Certified,
}

#[cfg(target_os = "linux")]
fn default_wayland_support() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|v| v == "wayland")
        .unwrap_or(false)
}

#[derive(Serialize, Deserialize, Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct Config {
    #[arg(long, help = "Access code")]
    pub access_code: Option<String>,
    #[arg(long, default_value = "0.0.0.0", help = "Bind address")]
    pub bind_address: IpAddr,
    #[arg(long, default_value = "1701", help = "Web port")]
    pub web_port: u16,
    #[cfg(target_os = "linux")]
    #[arg(
        long,
        help = "Try to use hardware acceleration through the Video Acceleration API."
    )]
    #[serde(default)]
    pub try_vaapi: bool,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[arg(long, help = "Try to use Nvidia's NVENC to encode the video via GPU.")]
    #[serde(default)]
    pub try_nvenc: bool,
    #[cfg(target_os = "macos")]
    #[arg(
        long,
        help = "Try to use hardware acceleration through the VideoToolbox API."
    )]
    #[serde(default)]
    pub try_videotoolbox: bool,
    #[cfg(target_os = "windows")]
    #[arg(
        long,
        help = "Try to use hardware acceleration through the MediaFoundation API."
    )]
    #[serde(default)]
    pub try_mediafoundation: bool,
    #[arg(long, help = "Start Rylus server immediately on program start.")]
    #[serde(default)]
    pub auto_start: bool,
    #[arg(long, help = "Gui Theme")]
    pub gui_theme: Option<ThemeType>,
    #[arg(long, help = "Run Rylus without gui and start immediately.")]
    #[serde(default)]
    pub no_gui: bool,
    #[arg(
        long,
        help = "Run a headless self-test and exit (routine implemented in a follow-up)."
    )]
    #[serde(default)]
    pub self_test: bool,
    #[cfg(target_os = "linux")]
    #[arg(
        long,
        help = "Wayland/PipeWire Support (auto-detected from XDG_SESSION_TYPE)."
    )]
    #[serde(default = "default_wayland_support", skip_serializing)]
    pub wayland_support: bool,

    #[arg(long, help = "Print template of index.html served by Rylus.")]
    #[serde(skip)]
    pub print_index_html: bool,
    #[arg(long, help = "Print access.html served by Rylus.")]
    #[serde(skip)]
    pub print_access_html: bool,
    #[arg(long, help = "Print style.css served by Rylus.")]
    #[serde(skip)]
    pub print_style_css: bool,
    #[arg(long, help = "Print lib.js served by Rylus.")]
    #[serde(skip)]
    pub print_lib_js: bool,

    #[arg(
        long,
        help = "Use custom template of index.html to be served by Rylus."
    )]
    #[serde(skip)]
    pub custom_index_html: Option<PathBuf>,
    #[arg(long, help = "Use custom access.html to be served by Rylus.")]
    #[serde(skip)]
    pub custom_access_html: Option<PathBuf>,
    #[arg(long, help = "Use custom style.css to be served by Rylus.")]
    #[serde(skip)]
    pub custom_style_css: Option<PathBuf>,
    #[arg(long, help = "Use custom lib.js to be served by Rylus.")]
    #[serde(skip)]
    pub custom_lib_js: Option<PathBuf>,

    #[arg(long, help = "Print shell completions for given shell.")]
    #[serde(skip)]
    pub completions: Option<clap_complete::Shell>,

    #[arg(long, help = "Print the roff-formatted man page to stdout.")]
    #[serde(skip)]
    pub print_man_page: bool,

    #[arg(long, help = "TLS mode: disabled, auto, certified")]
    #[serde(default)]
    pub tls_mode: Option<String>,
    #[arg(long, help = "Path to TLS certificate file")]
    #[serde(default)]
    pub tls_cert_path: Option<String>,
    #[arg(long, help = "Path to TLS private key file")]
    #[serde(default)]
    pub tls_key_path: Option<String>,
    #[arg(long, help = "Disable mDNS advertisement")]
    #[serde(default)]
    pub no_mdns: bool,
}

impl Config {
    pub fn resolve_tls_mode(&self) -> TlsMode {
        match self.tls_mode.as_deref() {
            Some("disabled") | Some("off") | Some("none") => TlsMode::Disabled,
            Some("certified") => TlsMode::Certified,
            Some("auto") | None => TlsMode::Auto,
            _ => TlsMode::Auto,
        }
    }
}

pub fn read_config() -> Option<Config> {
    if let Some(mut config_path) = dirs::config_dir() {
        config_path.push("rylus");
        config_path.push("rylus.toml");
        match fs::read_to_string(&config_path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("Failed to read configuration file: {}", e);
                    None
                }
            },
            Err(err) => {
                match err.kind() {
                    std::io::ErrorKind::NotFound => {
                        debug!("Failed to read configuration file: {}", err)
                    }
                    _ => warn!("Failed to read configuration file: {}", err),
                }
                None
            }
        }
    } else {
        None
    }
}

pub fn write_config(conf: &Config) {
    match dirs::config_dir() {
        Some(mut config_path) => {
            config_path.push("rylus");
            if !config_path.exists() {
                if let Err(err) = fs::create_dir_all(&config_path) {
                    warn!("Failed create directory for configuration: {}", err);
                    return;
                }
            }
            config_path.push("rylus.toml");
            match toml::to_string_pretty(&conf) {
                Ok(toml_string) => {
                    if let Err(err) = fs::write(config_path, &toml_string) {
                        warn!("Failed to write configuration file: {}", err);
                    }
                }
                Err(err) => {
                    warn!("Failed to encode config to toml: {}", err);
                }
            }
        }
        None => {
            warn!("Failed to find configuration directory!");
        }
    }
}

pub fn get_config() -> Config {
    let args: Vec<String> = std::env::args().collect();
    let mut config = if let Some(mut config) = read_config() {
        // The first element of args is always the binary name, so len > 1 means
        // the user passed actual command-line arguments that should override the config file.
        if args.len() > 1 {
            config.update_from(&args);
        }
        config
    } else {
        Config::parse()
    };
    if config.web_port < 1024 {
        warn!(
            "Port {} is in the privileged range (< 1024) and may require elevated permissions.",
            config.web_port
        );
    }
    // Auto-detect Wayland support at runtime unless the user explicitly passed
    // --wayland-support on the CLI to override.
    #[cfg(target_os = "linux")]
    {
        let explicitly_set = args.iter().any(|a| a == "--wayland-support");
        if !explicitly_set {
            config.wayland_support = default_wayland_support();
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let config = Config::parse_from::<_, &str>(["rylus"]);
        assert_eq!(config.web_port, 1701);
        assert_eq!(config.bind_address, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert!(config.access_code.is_none());
        assert!(!config.auto_start);
        assert!(!config.no_gui);
    }

    #[test]
    fn config_parse_cli_args() {
        let config = Config::parse_from([
            "rylus",
            "--access-code",
            "secret123",
            "--bind-address",
            "127.0.0.1",
            "--web-port",
            "8080",
            "--auto-start",
            "--no-gui",
        ]);
        assert_eq!(config.access_code.as_deref(), Some("secret123"));
        assert_eq!(config.bind_address, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(config.web_port, 8080);
        assert!(config.auto_start);
        assert!(config.no_gui);
    }

    #[test]
    fn config_toml_roundtrip() {
        let config = Config::parse_from(["rylus", "--access-code", "mycode", "--web-port", "9999"]);
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.access_code.as_deref(), Some("mycode"));
        assert_eq!(back.web_port, 9999);
    }

    #[test]
    fn config_toml_minimal() {
        // A minimal TOML with only required fields should parse
        let toml_str = r#"
bind_address = "0.0.0.0"
web_port = 1701
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.web_port, 1701);
        assert!(config.access_code.is_none());
    }

    #[test]
    fn config_toml_with_access_code() {
        let toml_str = r#"
bind_address = "192.168.1.100"
web_port = 3000
access_code = "hunter2"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.access_code.as_deref(), Some("hunter2"));
        assert_eq!(
            config.bind_address,
            "192.168.1.100".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn config_toml_invalid_address_fails() {
        let toml_str = r#"
bind_address = "not.an.ip"
web_port = 1701
"#;
        let result = toml::from_str::<Config>(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn config_toml_ipv6_address() {
        let toml_str = r#"
bind_address = "::1"
web_port = 1701
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.bind_address.is_ipv6());
    }

    #[test]
    fn theme_type_default_is_greybird() {
        assert_eq!(ThemeType::default(), ThemeType::Greybird);
    }

    #[test]
    fn theme_type_to_index_from_index_roundtrip() {
        for (i, theme) in ThemeType::themes().iter().enumerate() {
            assert_eq!(theme.to_index(), i as i32);
            assert_eq!(ThemeType::from_index(i as i32), *theme);
        }
    }

    #[test]
    fn theme_type_from_index_clamps() {
        let t = ThemeType::from_index(-1);
        assert_eq!(t, ThemeType::themes()[0]);
        let t = ThemeType::from_index(100);
        assert_eq!(t, *ThemeType::themes().last().unwrap());
    }

    #[test]
    fn theme_type_name() {
        assert_eq!(ThemeType::Dark.name(), "Dark");
        assert_eq!(ThemeType::Metro.name(), "Metro");
    }

    #[test]
    fn theme_list_length() {
        assert_eq!(ThemeType::themes().len(), 8);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn config_toml_with_linux_flags() {
        let toml_str = r#"
bind_address = "0.0.0.0"
web_port = 1701
try_vaapi = true
try_nvenc = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.try_vaapi);
        assert!(!config.try_nvenc);
    }

    #[test]
    fn config_parse_gui_theme() {
        let config = Config::parse_from(["rylus", "--gui-theme", "dark"]);
        assert_eq!(config.gui_theme, Some(ThemeType::Dark));
    }

    #[test]
    fn config_toml_skips_transient_fields() {
        let config = Config::parse_from(["rylus", "--print-index-html"]);
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("print_index_html"));
        assert!(!toml_str.contains("custom_index_html"));
    }

    #[test]
    fn tls_mode_defaults_to_auto() {
        // Default is now Auto — self-signed cert generated on first run.
        // Users must explicitly pass --tls-mode disabled to opt out.
        let config = Config::parse_from::<_, &str>(["rylus"]);
        assert_eq!(config.resolve_tls_mode(), TlsMode::Auto);
    }

    #[test]
    fn tls_mode_explicit_disabled() {
        let config = Config::parse_from(["rylus", "--tls-mode", "disabled"]);
        assert_eq!(config.resolve_tls_mode(), TlsMode::Disabled);
    }

    #[test]
    fn tls_mode_auto() {
        let config = Config::parse_from(["rylus", "--tls-mode", "auto"]);
        assert_eq!(config.resolve_tls_mode(), TlsMode::Auto);
    }

    #[test]
    fn tls_mode_certified() {
        let config = Config::parse_from(["rylus", "--tls-mode", "certified"]);
        assert_eq!(config.resolve_tls_mode(), TlsMode::Certified);
    }

    #[test]
    fn tls_mode_unknown_falls_back_to_auto() {
        let config = Config::parse_from(["rylus", "--tls-mode", "bogus"]);
        assert_eq!(config.resolve_tls_mode(), TlsMode::Auto);
    }

    #[test]
    fn no_mdns_defaults_false() {
        let config = Config::parse_from::<_, &str>(["rylus"]);
        assert!(!config.no_mdns);
    }

    #[test]
    fn no_mdns_flag() {
        let config = Config::parse_from(["rylus", "--no-mdns"]);
        assert!(config.no_mdns);
    }

    #[test]
    fn tls_cert_key_paths() {
        let config = Config::parse_from([
            "rylus",
            "--tls-cert-path",
            "/cert.pem",
            "--tls-key-path",
            "/key.pem",
        ]);
        assert_eq!(config.tls_cert_path.as_deref(), Some("/cert.pem"));
        assert_eq!(config.tls_key_path.as_deref(), Some("/key.pem"));
    }
}
