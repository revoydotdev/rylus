# Security Review: Path to 1.0

**Date:** 2026-06-26  
**Scope:** Current HEAD (`revoy-sim/ryl-security-review`).  The ROADMAP calls for a "diff review since v0.15.0", but that tag does not exist in the repository (tags jump from v0.11.4 to v0.16.1). This review therefore covers the current state of the security-relevant modules in full, with additional attention to the commits introduced between v0.16.1 and HEAD (see §6 for notes on which changes are new).  
**Version declared in Cargo.toml:** 0.17.0  
**Threat model:** Local-area-network screen-share tool. The adversary is a device on the same LAN, or a local user on the same machine. Internet exposure is explicitly out of scope for 1.0.

---

## Files examined

| File | Purpose |
|---|---|
| `crates/rylus-server/src/web.rs` | HTTP server, auth, rate limiting, session store, WS upgrade |
| `crates/rylus-server/src/tls.rs` | Certificate generation and loading |
| `crates/rylus-server/src/rylus.rs` | Server startup, TLS mode dispatch |
| `crates/rylus-server/src/session.rs` | WebSocket session handler (video/input, not HTTP sessions) |
| `crates/rylus-core/src/config.rs` | Config struct, TLS mode, file I/O |
| `crates/rylus-server/Cargo.toml` | Dependency versions |
| `Cargo.toml` (workspace) | Shared dep pinning |
| `Cargo.lock` | Resolved crate versions |
| `rylus_tls.sh` | Legacy TLS helper script |
| `ROADMAP.md` | 1.0 exit criteria |
| `CONTRIBUTING.md` | Contribution conventions |

---

## Findings

Severity scale: **Critical** (remote code execution, auth bypass with no preconditions) → **High** (significant data exposure or auth weakening with low effort) → **Medium** (weakens a stated security control; requires additional conditions to exploit) → **Low** (defence-in-depth gap, no direct exploitability).

---

### F-1 · High — TLS private key written to world-readable `/tmp`

**Location:** `crates/rylus-server/src/rylus.rs` line 68–69

```rust
TlsMode::Auto => {
    let cert_dir = std::path::PathBuf::from("/tmp").join("rylus");
    match crate::tls::load_or_generate_cert(
        &cert_dir.join("cert.der"),
        &cert_dir.join("key.der"),
    ) { ... }
}
```

**Behaviour:** On first run, `key.der` is written to `/tmp/rylus/key.der`. `/tmp` is readable by all local users on Linux (the sticky bit prevents deletion but not reading; the `rylus/` subdirectory inherits the same permissions unless explicitly restricted).

**Impact:** Any local user or process can read the private key and conduct a TLS man-in-the-middle attack against the LAN stream. Given that the browser already displays a cert warning for self-signed certs, the practical bar for this attack is low — the attacker needs only to intercept traffic, not to trick the user into ignoring an extra warning.

**Recommendation:** Store the auto-generated key in `$XDG_DATA_HOME/rylus/` or `$HOME/.config/rylus/` (the same directory already used for `rylus.toml`) and create the directory with mode `0700` before writing. The same path logic already exists in `config::write_config`.

---

### F-2 · Medium — Self-signed cert generated without Subject Alternative Names

**Location:** `crates/rylus-server/src/tls.rs` lines 6–11

```rust
pub fn generate_self_signed_cert() -> Result<(Vec<u8>, Vec<u8>), rcgen::Error> {
    let key_pair = rcgen::KeyPair::generate()?;
    let cert_params = rcgen::CertificateParams::default();
    let cert = cert_params.self_signed(&key_pair)?;
    Ok((cert.der().to_vec(), key_pair.serialize_der()))
}
```

**Behaviour:** `CertificateParams::default()` produces a cert with no Subject Alternative Names (SANs). Chrome (since version 58, 2017) and Edge reject connections where the presented certificate has no SAN matching the server address, even for self-signed certs with a matching CN. Firefox enforces the same rule in modern versions.

**Impact:** TLS Auto mode generates a cert that modern browsers will refuse to accept regardless of the user clicking through the "Advanced / proceed anyway" warning. The browser blocks the connection entirely with `NET::ERR_CERT_COMMON_NAME_INVALID`. This effectively makes TLS unusable without user configuration of `--tls-mode certified` — a worse outcome than it appears, since users may fall back to `--tls-mode disabled`.

**Recommendation:** Populate `subject_alt_names` in `CertificateParams` before signing. At minimum add the machine's hostname and the loopback addresses; ideally enumerate LAN IPs. The `hostname` crate is already in `Cargo.toml`; `rcgen::SanType::DnsName` and `rcgen::SanType::IpAddress` cover the cases. Example:

```rust
use rcgen::{CertificateParams, SanType};
let mut cert_params = CertificateParams::default();
cert_params.subject_alt_names = vec![
    SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
    SanType::IpAddress(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
    // add discovered LAN IPs / hostname here
];
```

---

### F-3 · Medium — Session cookie missing the `Secure` attribute

**Location:** `crates/rylus-server/src/web.rs` line 440

```rust
format!(
    "rylus_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400"
)
```

**Behaviour:** The `Secure` attribute is absent. Without it, the browser will transmit the session cookie over a plain-HTTP connection if the server is reachable via both HTTPS and HTTP (or if TLS falls back to plain TCP — see F-4).

**Impact:** If the server falls back to plain TCP for any reason, an active LAN eavesdropper can capture the session cookie and replay it to the HTTPS endpoint (if TLS is later restored) or to the plain-TCP endpoint. The combination of F-3 and F-4 produces a session-hijack path with no server-side mitigation.

**Recommendation:** Add `; Secure` to the `Set-Cookie` header. Since the server operates in TLS-Auto by default, this attribute should always be present. If `--tls-mode disabled` is explicitly requested, the server should warn loudly in the startup log that cookies are not protected, but the attribute itself can still be set (browsers ignore it for HTTP anyway, but it signals intent and future-proofs against HTTP→HTTPS upgrade paths).

---

### F-4 · Medium — TLS failure silently falls back to plaintext

**Location:** `crates/rylus-server/src/rylus.rs` lines 73–78

```rust
Err(err) => {
    warn!("TLS auto-setup failed, falling back to plain TCP: {err}");
    None
}
```

**Behaviour:** If certificate generation or disk write fails (e.g., `/tmp` is `noexec`/`nodev`, disk full, or permission error), the server continues on plain TCP after logging a single `warn!`. The GUI does not surface this state; headless installations produce only a log line.

**Impact:** Users who expect TLS protection receive none. The log line may be missed in a systemd journal or scrolled past in a terminal. This is a silent security downgrade.

**Recommendation:** At minimum, add a second warning log line at server start that reports the actual running transport (`INFO: Server running without TLS — all traffic is unencrypted`). Consider making the TLS failure a hard error in non-interactive (headless/systemd) mode, or expose a `/health` endpoint that advertises the TLS status so monitoring can detect it.

---

### F-5 · Medium — Argon2 default parameters below OWASP minimum for memory

**Location:** `crates/rylus-server/src/web.rs` lines 316–323

```rust
fn hash_access_code(code: &str) -> String {
    let salt = argon2::password_hash::SaltString::generate(&mut rand::thread_rng());
    let argon2 = Argon2::default();  // Argon2id, m=19456 KiB, t=2, p=1
    ...
}
```

**Behaviour:** `Argon2::default()` in the `argon2` 0.5 crate uses Argon2id with `m_cost = 19456 KiB` (≈19 MiB), `t_cost = 2`, `p_cost = 1`. The 2023 OWASP cheat sheet recommends a minimum of `m = 47104 KiB` (46 MiB) with `t = 1` and a preferred profile of `m = 65536 KiB` (64 MiB) with `t = 3`.

**Scope of risk:** The hash is computed once on server startup, so higher parameters do not affect per-request latency. The risk is offline brute-force if an attacker reads the config file (`~/.config/rylus/rylus.toml`, where the plaintext access code is already stored — see F-6). The argon2 protection matters only in the scenario where the attacker can read the config but cannot directly read the access code field; that scenario is narrow.

**Recommendation:** Raise parameters explicitly rather than relying on `default()`. Suggested: `m_cost = 65536, t_cost = 3, p_cost = 1` with a comment citing OWASP. This is a server-side-only change with no user-visible latency impact (startup hashing of one password).

```rust
let params = argon2::Params::new(65536, 3, 1, None)
    .expect("argon2 params are valid");
let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
```

---

### F-6 · Low — Plaintext access code stored in config and returned by API

**Location:** `crates/rylus-server/src/web.rs` line 649; `crates/rylus-core/src/config.rs` line 73

**Behaviour:** The `Config` struct stores `access_code: Option<String>` in the clear. This value is persisted to `~/.config/rylus/rylus.toml` and is returned by `GET /api/config` (behind session authentication). The argon2 hash exists only in the server's runtime state for verifying remote auth attempts; local file read bypasses it entirely.

**Assessment:** For a LAN tool where the server operator controls the machine, this is an expected design trade-off (the UI must display the code so the user can share it with tablet users). It is not a vulnerability given the threat model, but it should be documented so operators understand that file read access equals key access.

**Recommendation:** Add a note in `docs/` or the README stating that `rylus.toml` contains the access code in plaintext; operators who want to protect it should apply filesystem permissions (e.g., `chmod 600 ~/.config/rylus/rylus.toml`). No code change required.

---

### F-7 · Low — Rate limiter HashMap is unbounded

**Location:** `crates/rylus-server/src/web.rs` lines 118–166

**Behaviour:** The `RateLimiter` stores one `Vec<Instant>` entry per source IP address. Entries are lazily cleaned up only when that IP's slot is consulted. An attacker cycling through many source IPs would grow the HashMap indefinitely. For a LAN tool this is a marginal concern — a /24 network yields at most 254 IPs — but an IPv6 LAN or a scan from a /16 could accumulate thousands of entries.

**Recommendation:** Add an eviction pass: cap the map at some maximum size (e.g., 4096 entries) or periodically remove entries where all timestamps have aged past `RATE_LIMIT_WINDOW`. Alternatively, a `DashMap` with an LRU front-end would handle this naturally.

---

### F-8 · Low — Legacy `rylus_tls.sh` still ships, conflicts with native TLS

**Location:** `rylus_tls.sh`

**Behaviour:** The script generates a 4096-bit RSA cert (`--nodes`, no passphrase) and proxies traffic through `hitch`. It predates the native TLS implementation. It is not referenced from the README, ROADMAP, or current documentation, but it ships in the repository root and may be discovered by users or packagers as an "official" TLS setup path.

**Issues:**
- `--nodes` stores the private key unprotected (same as the native path, but combined with a bare `/` home directory or AUR package, this may be unexpected).
- The hitch dependency is not declared anywhere; silently fails if hitch is absent.
- RSA 4096 is computationally heavier than the ECDSA P-256 generated by rcgen; no practical security benefit for a LAN tool.
- The `function` keyword is bash-specific but the shebang is `#!/usr/bin/env sh`; on dash/ash `function` is a syntax error.

**Recommendation:** Remove `rylus_tls.sh` from the repository now that native TLS is implemented. If a reference to the legacy path is needed, move it to an archived wiki page or a comment in git history.

---

### F-9 · Low — `TlsMode::default()` and `resolve_tls_mode(None)` diverge

**Location:** `crates/rylus-core/src/config.rs` lines 54–60, 171–178

**Behaviour:** `TlsMode` derives `Default`, which yields `TlsMode::Disabled` (the first variant). But `Config::resolve_tls_mode()` maps `tls_mode: None` to `TlsMode::Auto`. Any code that calls `TlsMode::default()` directly would get `Disabled` — the opposite of the intended behaviour.

**Recommendation:** Either change `#[default]` to `Auto`, or remove the `Default` derive and add a comment that `resolve_tls_mode` is the correct API. This prevents a future caller from accidentally getting `Disabled` by relying on `TlsMode::default()`.

---

## Summary table

| ID | Severity | Area | One-line summary |
|---|---|---|---|
| F-1 | High | TLS | Private key in world-readable `/tmp` |
| F-2 | Medium | TLS | No SANs — modern browsers refuse the cert |
| F-3 | Medium | Session | Cookie missing `Secure` flag |
| F-4 | Medium | TLS | Silent fallback to plaintext on cert error |
| F-5 | Medium | Argon2 | Default params below OWASP minimum memory cost |
| F-6 | Low | Access code | Plaintext code in config file — expected but undocumented |
| F-7 | Low | Rate limit | Rate limiter HashMap is unbounded |
| F-8 | Low | TLS | Legacy `rylus_tls.sh` conflicts with and predates native TLS |
| F-9 | Low | TLS | `TlsMode::default()` diverges from `resolve_tls_mode(None)` |

---

## Items assessed as solid

- **Argon2 algorithm and variant:** Argon2id with a random salt per hash is correct. `SaltString::generate` uses `OsRng` internally; the salt is adequate. The hash format (PHC string) is stored correctly and parsed with `PasswordHash::new` before verification.
- **Session token entropy:** 32 characters from a 36-symbol alphabet yields ≈165 bits of entropy. `rand::thread_rng()` on Rust stable is seeded from `OsRng`; it is a CSPRNG.
- **Session invalidation on code rotation:** `SessionStore::clear()` is called in `handle_post_config` whenever `access_code` changes (commit 617c661). All connected sessions are forced to re-authenticate.
  - *Correction (2026-07-14):* the original review missed that the in-memory verification hash was **not** updated on rotation — the old code stayed valid (and the new code was rejected) until restart. Fixed by moving the hash behind a `RwLock` swapped inside `handle_post_config`; verified end-to-end (old session 401s, old code rejected, new code authenticates, no restart).
- **WebSocket Origin check (added in 617c661):** The `ws_origin_matches_host` function correctly strips the scheme and path from the `Origin` header and compares the authority to the `Host` header. Absent `Origin` is intentionally permitted for non-browser clients. The auth check runs before the Origin check, so an unauthenticated cross-origin request is rejected as `401` before reaching the Origin logic.
- **Rate limiter logic:** The lockout check uses `Instant` (monotonic clock); there is no TOCTOU issue. IPv4 and IPv4-mapped IPv6 addresses are handled as distinct `IpAddr` variants, preventing easy bypass by address-family switching. Different IPs are correctly isolated.
- **Cookie attributes present:** `HttpOnly` and `SameSite=Strict` are set on the session cookie, which prevents XSS-based cookie theft and CSRF respectively.
- **TLS protocol:** `rustls 0.23` with the `ring` backend defaults to TLS 1.2 and 1.3 with strong cipher suites. `with_no_client_auth()` is correct for this use case.
- **Key algorithm:** `rcgen::KeyPair::generate()` defaults to ECDSA P-256, which is appropriate.

---

## What could not be verified from code alone

- **rcgen cert validity period:** `CertificateParams::default()` sets `not_after` to a value determined by the rcgen library version (0.13.2). The exact expiry date is not visible in the source; it may be a far-future date. This did not affect any finding above but should be confirmed by running `openssl x509 -in cert.der -inform DER -noout -dates` on a generated cert.
  - *Resolved (2026-07-14):* certs are now generated with explicit SANs (hostname, `<hostname>.local`, `localhost`, all local IPs) and a 730-day validity window, within Apple's 825-day acceptance limit. Auto-mode filenames were bumped (`cert-v2.der`/`key-v2.der`) so pre-existing SAN-less pairs regenerate.
- **Browser accept/reject of the cert in practice:** F-2 is based on Chrome's documented behaviour since 2017. The exact browser behaviour for the specific cert produced by rcgen 0.13.2 without SANs was not tested interactively.
- **Hitch TLS proxy version and CVE status** (F-8): not assessed since the script is recommended for removal.
