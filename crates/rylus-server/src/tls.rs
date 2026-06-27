use std::path::Path;
#[cfg(unix)]
use std::io::Write as _;
use tracing::info;

/// Generate a self-signed certificate and private key (PKCS#8 DER).
/// Returns `(cert_der, key_der)`.
pub fn generate_self_signed_cert() -> Result<(Vec<u8>, Vec<u8>), rcgen::Error> {
    let key_pair = rcgen::KeyPair::generate()?;
    let cert_params = rcgen::CertificateParams::default();
    let cert = cert_params.self_signed(&key_pair)?;
    Ok((cert.der().to_vec(), key_pair.serialize_der()))
}

/// Build a [`rustls::ServerConfig`] from DER-encoded cert chain and private key.
pub fn build_server_config(
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
) -> Result<rustls::ServerConfig, rustls::Error> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    let certs = vec![CertificateDer::from(cert_der)];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));

    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
}

/// Load an existing cert/key pair from disk, or generate a self-signed pair and
/// persist it. Returns a ready-to-use [`rustls::ServerConfig`].
pub fn load_or_generate_cert(
    cert_path: &Path,
    key_path: &Path,
) -> Result<rustls::ServerConfig, Box<dyn std::error::Error + Send + Sync>> {
    let (cert_der, key_der) = if cert_path.exists() && key_path.exists() {
        info!("Loading TLS certificate from {:?}", cert_path);
        (std::fs::read(cert_path)?, std::fs::read(key_path)?)
    } else {
        info!("No existing TLS certificate found — generating self-signed cert...");
        let (cert, key) = generate_self_signed_cert()?;
        if let Some(parent) = cert_path.parent() {
            std::fs::create_dir_all(parent)?;
            // Restrict the directory to the current user only (rwx------).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        // Public cert — 0644 is fine; the private key must be owner-read only.
        std::fs::write(cert_path, &cert)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(key_path)?
                .write_all(&key)?;
        }
        #[cfg(not(unix))]
        std::fs::write(key_path, &key)?;
        info!(
            "Saved TLS certificate to {:?}, key to {:?}",
            cert_path, key_path
        );
        (cert, key)
    };

    Ok(build_server_config(cert_der, key_der)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn _ensure_crypto_provider_installed() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn generate_self_signed_cert_returns_non_empty_der() {
        _ensure_crypto_provider_installed();
        let (cert, key) = generate_self_signed_cert().unwrap();
        assert!(!cert.is_empty(), "cert DER must not be empty");
        assert!(!key.is_empty(), "key DER must not be empty");
    }

    #[test]
    fn generate_self_signed_cert_builds_valid_config() {
        _ensure_crypto_provider_installed();
        let (cert, key) = generate_self_signed_cert().unwrap();
        let result = build_server_config(cert, key);
        assert!(result.is_ok(), "should build a valid ServerConfig");
    }

    #[test]
    fn load_or_generate_cert_roundtrip() {
        _ensure_crypto_provider_installed();
        let dir = std::env::temp_dir().join("rylus-tls-test-roundtrip");
        let cert_path = dir.join("cert.der");
        let key_path = dir.join("key.der");

        let _ = std::fs::remove_dir_all(&dir);

        let config1 = load_or_generate_cert(&cert_path, &key_path);
        assert!(config1.is_ok(), "first call should succeed");
        assert!(cert_path.exists(), "cert file should be created");
        assert!(key_path.exists(), "key file should be created");

        let config2 = load_or_generate_cert(&cert_path, &key_path);
        assert!(config2.is_ok(), "second call (load) should succeed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Verify that the generated key file is owner-read-only (0600) and that
    /// the containing directory is private (0700) on Unix.
    #[cfg(unix)]
    #[test]
    fn generated_key_has_private_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        _ensure_crypto_provider_installed();
        let dir = std::env::temp_dir().join("rylus-tls-test-perms");
        let cert_path = dir.join("cert.der");
        let key_path = dir.join("key.der");

        let _ = std::fs::remove_dir_all(&dir);

        let result = load_or_generate_cert(&cert_path, &key_path);
        assert!(result.is_ok(), "cert generation should succeed");

        let key_mode = std::fs::metadata(&key_path)
            .expect("key file must exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(key_mode, 0o600, "key file must be mode 0600, got {:o}", key_mode);

        let dir_mode = std::fs::metadata(&dir)
            .expect("cert dir must exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700, "cert dir must be mode 0700, got {:o}", dir_mode);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_server_config_rejects_empty_inputs() {
        _ensure_crypto_provider_installed();
        let result = build_server_config(vec![], vec![]);
        assert!(result.is_err(), "empty cert/key should fail");
    }
}
