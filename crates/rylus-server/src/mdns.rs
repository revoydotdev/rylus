use std::fmt;

use mdns_sd::{ServiceDaemon, ServiceInfo};

pub const SERVICE_TYPE: &str = "_rylus._tcp.local.";
const PROTOCOL_VERSION: &str = "3";

/// mDNS publisher for advertising the Rylus server on the local network.
///
/// Publishes a `_rylus._tcp` service so clients can discover the server
/// without manual IP entry or QR codes.
pub struct MdnsPublisher {
    daemon: Option<ServiceDaemon>,
    fullname: Option<String>,
}

#[derive(Debug)]
pub struct MdnsError(String);

impl fmt::Display for MdnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mDNS error: {}", self.0)
    }
}

impl std::error::Error for MdnsError {}

impl MdnsPublisher {
    /// Create a new (unregistered) mDNS publisher.
    pub fn new() -> Self {
        Self {
            daemon: None,
            fullname: None,
        }
    }

    /// Whether this publisher has an active registration.
    pub fn is_registered(&self) -> bool {
        self.daemon.is_some() && self.fullname.is_some()
    }

    /// Register the Rylus service on the local network.
    ///
    /// # Arguments
    /// * `port` - The TCP port the web server is listening on.
    /// * `hostname` - Optional custom hostname for the service name. Falls back to system hostname.
    ///
    /// # Errors
    /// Returns `MdnsError` if the daemon cannot be created or the service info is invalid.
    /// These errors are **non-fatal** — callers should log and continue.
    pub fn register(&mut self, port: u16, hostname: Option<&str>) -> Result<(), MdnsError> {
        if self.is_registered() {
            self.unregister();
        }

        let daemon = ServiceDaemon::new().map_err(|e| MdnsError(e.to_string()))?;

        // Append a short PID-derived suffix so two Rylus servers on the same
        // host or two hosts with identical `hostname` don't collide on the
        // `_rylus._tcp.local.` namespace.
        let suffix = collision_suffix();
        let instance_name = match hostname {
            Some(h) => format!("Rylus {h} {suffix}"),
            None => format!("Rylus {} {}", get_hostname(), suffix),
        };

        let host_name = format!("{}.local.", get_hostname());

        let properties: &[(&str, &str)] = &[("version", PROTOCOL_VERSION)];

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &host_name,
            "",
            port,
            properties,
        )
        .map_err(|e| MdnsError(e.to_string()))?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();

        daemon
            .register(service_info)
            .map_err(|e| MdnsError(e.to_string()))?;

        self.daemon = Some(daemon);
        self.fullname = Some(fullname);

        Ok(())
    }

    /// Unregister the service and shut down the mDNS daemon.
    ///
    /// Safe to call multiple times. No-op if not currently registered.
    pub fn unregister(&mut self) {
        if let (Some(fullname), Some(daemon)) = (self.fullname.take(), self.daemon.take()) {
            let _ = daemon.unregister(&fullname);
            daemon.shutdown().ok();
        }
    }
}

impl Default for MdnsPublisher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MdnsPublisher {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Get the system hostname, or "unknown" if unavailable.
fn get_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// 4-character lowercase-alphanumeric suffix derived from PID + a random
/// nibble. Stable for the lifetime of the process, so clients can re-resolve
/// after a reconnect, but unique enough to survive a collision with another
/// Rylus instance on the same LAN.
fn collision_suffix() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static CACHED: AtomicU32 = AtomicU32::new(0);
    let existing = CACHED.load(Ordering::Relaxed);
    if existing != 0 {
        return format_suffix(existing);
    }
    // PID keeps the suffix stable for this server's lifetime. Mix in a few
    // bits from the process start time to reduce reuse after PID wrap-around.
    let pid = std::process::id();
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let mixed = pid.wrapping_mul(0x9E37_79B1).wrapping_add(jitter) | 1;
    CACHED.store(mixed, Ordering::Relaxed);
    format_suffix(mixed)
}

fn format_suffix(mixed: u32) -> String {
    // 4 base-36 chars gives ~1.6M distinct values — plenty for LAN uniqueness.
    let mut n = mixed;
    let mut out = String::with_capacity(4);
    for _ in 0..4 {
        let d = (n % 36) as u8;
        let c = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        out.push(c as char);
        n /= 36;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_type_is_correct() {
        assert_eq!(SERVICE_TYPE, "_rylus._tcp.local.");
    }

    #[test]
    fn build_service_info_with_explicit_hostname() {
        let instance_name = "Rylus myhost";
        let host_name = "myhost.local.";
        let port = 1701;
        let properties: &[(&str, &str)] = &[("version", "3")];

        let info = ServiceInfo::new(SERVICE_TYPE, instance_name, host_name, "", port, properties);

        assert!(info.is_ok(), "ServiceInfo creation should succeed");
        let info = info.unwrap().enable_addr_auto();
        assert_eq!(info.get_port(), port);
        assert!(info.get_fullname().contains("Rylus myhost"));
        assert!(info.get_fullname().contains(SERVICE_TYPE));
    }

    #[test]
    fn build_service_info_with_auto_hostname() {
        let hostname = get_hostname();
        let instance_name = format!("Rylus {hostname}");
        let host_name = format!("{hostname}.local.");
        let port = 8080;

        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &host_name,
            "",
            port,
            &[("version", "3")] as &[(&str, &str)],
        )
        .unwrap()
        .enable_addr_auto();

        assert_eq!(info.get_port(), port);
        assert!(info.get_fullname().contains(&instance_name));
    }

    #[test]
    fn service_info_txt_record_contains_version() {
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            "Rylus test",
            "test.local.",
            "",
            1234,
            &[("version", "3")] as &[(&str, &str)],
        )
        .unwrap()
        .enable_addr_auto();

        let props = info.get_properties();
        assert_eq!(
            props.get_property_val_str("version"),
            Some("3"),
            "TXT record must contain version=3"
        );
    }

    // ---- State management ----

    #[test]
    fn publisher_starts_unregistered() {
        let publisher = MdnsPublisher::new();
        assert!(!publisher.is_registered());
    }

    #[test]
    fn default_publisher_is_unregistered() {
        let publisher = MdnsPublisher::default();
        assert!(!publisher.is_registered());
    }

    #[test]
    fn register_and_unregister_lifecycle() {
        let mut publisher = MdnsPublisher::new();
        assert!(!publisher.is_registered());

        let unique_name = format!("Rylus-test-{}", std::process::id());
        let result = publisher.register(0, Some(&unique_name));
        assert!(
            result.is_ok(),
            "Register should succeed: {:?}",
            result.err()
        );
        assert!(publisher.is_registered());

        publisher.unregister();
        assert!(!publisher.is_registered());
    }

    #[test]
    fn unregister_is_idempotent() {
        let mut publisher = MdnsPublisher::new();
        publisher.unregister();
        publisher.unregister();
        assert!(!publisher.is_registered());
    }

    #[test]
    fn register_replaces_previous_registration() {
        let mut publisher = MdnsPublisher::new();
        let name1 = format!("Rylus-replace-{}", std::process::id());
        let name2 = format!("Rylus-replace2-{}", std::process::id());

        publisher.register(1701, Some(&name1)).unwrap();
        assert!(publisher.is_registered());

        publisher.register(1702, Some(&name2)).unwrap();
        assert!(publisher.is_registered());

        publisher.unregister();
        assert!(!publisher.is_registered());
    }

    // ---- Error handling ----

    #[test]
    fn mdns_error_display() {
        let err = MdnsError("daemon failed".to_string());
        assert_eq!(format!("{err}"), "mDNS error: daemon failed");
    }

    #[test]
    fn mdns_error_debug() {
        let err = MdnsError("test".to_string());
        let debug = format!("{err:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn get_hostname_returns_nonempty() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty(), "Hostname should not be empty");
    }

    #[test]
    fn collision_suffix_is_four_base36_chars() {
        let s = collision_suffix();
        assert_eq!(s.len(), 4);
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "suffix must be lowercase base-36: got {s}"
        );
    }

    #[test]
    fn collision_suffix_is_stable_across_calls() {
        let a = collision_suffix();
        let b = collision_suffix();
        assert_eq!(a, b, "suffix must be stable for the life of the process");
    }

    #[test]
    fn format_suffix_various_inputs() {
        assert_eq!(format_suffix(0).len(), 4);
        assert_eq!(format_suffix(u32::MAX).len(), 4);
    }
}
