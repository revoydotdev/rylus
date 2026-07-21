# Rylus Security Review

This document describes what is actually implemented in the codebase for the
security-relevant areas below, each verified by reading the current source in
this tree. It is not a claim of completeness or of formal audit; it is an
honest inventory of what exists, why it exists, and where it is weaker than
ideal.

## 1. Origin enforcement on WebSocket upgrade

**What's implemented:** The `/ws` handler in
`crates/rylus-server/src/web.rs` rejects a WebSocket upgrade with `403
Forbidden` if the request carries an `Origin` header whose authority
(host[:port], scheme stripped) does not match the request's `Host` header.
Requests with no `Origin` header at all are allowed through. The comparison
logic is `ws_origin_matches_host` (`crates/rylus-server/src/web.rs:279-301`);
the call site and rejection branch are in the `"/ws"` match arm
(`crates/rylus-server/src/web.rs:520-546`).

**Why:** This is a defense against cross-site WebSocket hijacking — a
malicious page loaded in a browser that already has a valid `rylus_session`
cookie for this origin (e.g. because the user is on the same LAN and
previously authenticated) should not be able to silently open a WebSocket
against the Rylus server from a different origin. Browsers always send
`Origin` on cross-origin fetch/WS attempts, so checking it here is effective
against browser-based attackers.

**Known trade-off:** Non-browser clients (curl, scripts, other native
clients, and some mDNS/discovery-driven connections) typically send no
`Origin` header, and those are intentionally allowed through regardless of
Host. This is a deliberate compatibility decision, not an oversight — it
means the Origin check only constrains browser-vectored attacks, not a
scripted client that omits the header on purpose. This behavior was
previously wired and confirmed working before this review pass; this review
did not re-run the wiring check, only reread the source.

## 2. Argon2 access-code hashing

**What's implemented:** Access codes (when configured) are hashed with
Argon2 before being stored in `WebServerConfig.access_code_hash`
(`crates/rylus-server/src/web.rs:783-806`, computed once in
`WebServerConfig::new`). Hashing uses `Argon2::default()` with a
per-hash random salt from `SaltString::generate(&mut rand::thread_rng())`
(`hash_access_code`, `crates/rylus-server/src/web.rs:316-323`). Verification
parses the stored PHC-format hash string with `PasswordHash::new` and calls
`Argon2::default().verify_password(...)` (`verify_access_code`,
`crates/rylus-server/src/web.rs:326-337`), which performs the comparison
internally in the `argon2` crate (constant-time by construction of the
crate's verification path, not a manual `==`).

`Argon2::default()` in the `argon2` crate (version `0.5.3`, pinned in
`Cargo.lock`, declared as `argon2 = "0.5"` in
`crates/rylus-server/Cargo.toml:27`) uses the Argon2id variant with the
crate's built-in default `Params` (as of 0.5.x: m_cost = 19456 KiB, t_cost =
2, p_cost = 1 — the RFC 9106 "second recommended" parameter set). These are
not overridden anywhere in this codebase; no custom `Params` or `Argon2::new`
call exists for the access code path.

**Why:** Argon2id with a random salt defeats rainbow-table/precomputation
attacks and (at these cost parameters) makes brute-forcing a short access
code by an attacker with the hash meaningfully more expensive than a fast
hash (SHA-256, etc.) would be. Using the crate's own verifier rather than a
manual byte comparison avoids introducing a timing side channel in the
comparison step.

**Observation:** The access code itself is operator-supplied and can be
short/low-entropy (it's meant to be typed on a TV remote or tablet). Argon2
raises the cost of offline brute force per guess but does not compensate for
a very low-entropy code; online guessing is instead bounded by the rate
limiter described below.

## 3. Rate limiting on `/auth`

**What's implemented:** `RateLimiter`
(`crates/rylus-server/src/web.rs:118-166`) tracks failed-attempt timestamps
per source IP in a `Mutex<HashMap<IpAddr, Vec<Instant>>>`. Constants:
`MAX_FAILED_ATTEMPTS = 5`, `RATE_LIMIT_WINDOW = 60s`, `LOCKOUT_DURATION =
30s` (`crates/rylus-server/src/web.rs:111-115`). `record_failure` prunes
timestamps older than the window and appends a new one
(`web.rs:158-165`). `lockout_remaining` prunes stale timestamps, and if
`MAX_FAILED_ATTEMPTS` or more remain within the window, returns
`Some(seconds_remaining)` computed from `LOCKOUT_DURATION` minus time since
the most recent failure (`web.rs:132-147`).

Wiring: the `POST /auth` handler checks `context.rate_limiter
.lockout_remaining(&addr.ip())` before doing any password work and, if
locked out, redirects with `303 See Other` plus a `Retry-After` header and an
inline `auth_error=rate_limited` query param, without touching the argon2
verifier at all (`web.rs:400-414`). On a failed code check it calls
`context.rate_limiter.record_failure(&addr.ip())` and redirects with either
`rate_limited` or `invalid` depending on whether the failure just tripped the
lockout (`web.rs:449-462`).

**Why:** This bounds online brute-force attempts against the access code to
5 tries per 60-second window per source IP, with a 30-second lockout once
tripped. Checking the lockout before calling into the argon2 verifier also
avoids doing the (deliberately expensive) hash computation for
already-blocked callers.

**Observation:** The limiter keys strictly on source `IpAddr`. Any NAT'd LAN
(many home/office networks) shares one IP across many devices, so the limit
is effectively per-NAT-egress, not per-attacker; conversely an attacker who
can spoof/rotate source addresses (unlikely on a LAN scenario this targets,
but worth naming) is not slowed at all. There is no persistence across
process restarts — `attempts` is an in-memory `HashMap` reset on every
server restart, so lockouts don't survive a crash/restart cycle.

## 4. TLS: self-signed certificate generation and mode selection

**What's implemented:** `crates/rylus-server/src/tls.rs` generates a
self-signed cert/key pair via the `rcgen` crate (version `0.13.2`, pinned in
`Cargo.lock`, declared `rcgen = { workspace = true }` in
`crates/rylus-server/Cargo.toml:38`): `generate_self_signed_cert` calls
`rcgen::KeyPair::generate()` and `CertificateParams::default().self_signed(&key_pair)`
(`tls.rs:6-11`) — i.e. `rcgen`'s default parameters (algorithm, validity
window, subject) are used unmodified; nothing here customizes SAN entries,
validity period, or key algorithm. `build_server_config` wraps the DER cert
and PKCS#8 key into a `rustls::ServerConfig` with `with_no_client_auth()`
(`tls.rs:14-26`), so there is no client-certificate/mTLS requirement.
`load_or_generate_cert` (`tls.rs:30-53`) loads an existing cert/key pair from
disk if both files exist, otherwise generates and persists a new pair via
plain `std::fs::write` with no explicit file permission hardening (no
`set_permissions`/`0600` — the files inherit the process umask).

Mode selection lives in `rylus-core`: `TlsMode` (`crates/rylus-core/src/config.rs:55`)
has three variants — `Disabled`, `Auto`, `Certified` — resolved by
`Config::resolve_tls_mode` (`crates/rylus-core/src/config.rs:177-183`) from
the `tls_mode` config string (`"disabled"|"off"|"none"` → Disabled,
`"certified"` → Certified, anything else including absent → Auto). The
actual decision is wired in `crates/rylus-server/src/rylus.rs:59-99`:
- `Disabled` logs a `warn!` that "access codes and input events travel in
  cleartext" and runs plain TCP (`rylus.rs:60-66`).
- `Auto` calls `load_or_generate_cert` against a fixed path,
  `/tmp/rylus/cert.der` and `/tmp/rylus/key.der` (`rylus.rs:68-72`) — falling
  back to plain TCP with a `warn!` if cert setup fails.
- `Certified` requires `tls_cert_path`/`tls_key_path` to both be set in
  config; if either is empty it warns and falls back to no TLS
  (`rylus.rs:80-97`).

**Why:** TLS terminates at the `tokio_rustls::TlsAcceptor` per accepted
connection in `crates/rylus-server/src/web.rs:920-934`, so once configured
it protects the access-code POST body, the session cookie, and all
subsequent WebSocket control/video traffic from passive LAN eavesdropping.
Self-signed generation means TLS is available out of the box with zero
operator setup (the default mode is `Auto`, confirmed by
`crates/rylus-core/src/config.rs:181` and the `tls_mode_defaults_to_auto`
test at `config.rs:423-427`).

**Observations (weaker than ideal):**
- Self-signed certs mean clients get no real chain-of-trust validation;
  browsers will show a certificate warning, and any MITM-capable attacker on
  the same LAN segment can present their own self-signed cert unless the
  client pins the original. This is an accepted trade-off for a
  self-hosted/LAN tool, but it is not the same security property as a
  CA-issued cert.
- The `Auto` mode's cert/key live under `/tmp/rylus/`, a world-readable
  temp directory on multi-user systems, and are written with
  `std::fs::write` without tightening permissions — on a shared machine
  another local user could potentially read the private key. `/tmp` is also
  typically cleared on reboot, so `Auto` regenerates on every reboot even
  though the code otherwise treats "file exists" as "reuse it."
- `Disabled` mode is silently accepted as a valid configuration (only a log
  warning, no hard refusal), and the default bind address is `0.0.0.0`
  (`crates/rylus-core/src/config.rs:280`), i.e. all interfaces — so a
  misconfigured `Disabled` deployment is reachable, in cleartext, from the
  entire LAN, not just localhost.

## 5. Session-token lifecycle

**What's implemented:** `SessionStore` (`crates/rylus-server/src/web.rs:169-220`)
holds a `Mutex<HashMap<String, Instant>>` mapping session token to creation
time. `SESSION_TTL = 24 * 3600` seconds (`web.rs:174`). `create_session`
generates a 32-character token by drawing 32 independent values from
`rand::thread_rng().gen_range(0..36)` and mapping each to `0-9a-z`
(`web.rs:191-206`) — i.e. roughly 32 * log2(36) ≈ 165 bits of entropy from a
non-cryptographic-but-CSPRNG-backed source (`rand::thread_rng()` is backed by
a CSPRNG, not a predictable PRNG). `is_valid` checks presence and
`elapsed() < SESSION_TTL`, removing the entry if expired (`web.rs:209-219`).

The token is delivered via `Set-Cookie: rylus_session=<token>; HttpOnly;
SameSite=Strict; Path=/; Max-Age=86400` on successful auth
(`web.rs:439-443`), and read back via `extract_session_token` which parses
the raw `Cookie` header for a `rylus_session=` entry
(`web.rs:304-313`). `HttpOnly` blocks JS access to the cookie (mitigates
token theft via XSS); `SameSite=Strict` blocks the cookie being sent on
cross-site navigations/requests (defense-in-depth alongside the Origin
check in §1). Every protected route (`/settings`, `/api/config` GET/POST,
`/`, `/ws`) re-derives `preauthed`/`authed` per request from the cookie
(`web.rs:353-359`, `466-473`), so there's no separate "logged in forever"
flag independent of the session store.

**Invalidation:** there is no explicit logout endpoint; sessions end via
`SESSION_TTL` expiry or via `SessionStore::clear()`, which is called when the
access code is rotated through `POST /api/config`
(`handle_post_config`, `web.rs:740-747`) — rotating the code wipes every
active session so all previously-authenticated clients must re-enter the new
code (`web.rs:744-745`, comment confirms this is intentional).

**Observations:**
- No cookie `Secure` attribute is set. On the `Disabled` TLS mode this is
  moot (everything is cleartext anyway), but even under TLS the cookie
  lacks `Secure`, so if the app is ever also reachable over plain HTTP on
  the same host/port (e.g. a future dual-listener) the session cookie could
  be sent in cleartext. Currently the server only listens on one accepted
  socket type at a time per the `TlsOrTcpStream` design, so this is latent
  rather than exploitable today, but it's worth flagging.
  - **Correction to be precise**: I did not find any HTTP↔HTTPS
    upgrade/redirect logic in `web.rs`; the server binds one listener and
    that listener is either fully TLS or fully plain TCP based on
    `tls_config` being `Some`/`None` at `run_server` startup
    (`web.rs:920-934`). So `Secure`'s absence is currently inert given the
    single-listener design, not a live gap — but adding a second listener
    later without adding `Secure` would reintroduce it.
- Tokens are never individually revocable — there's no per-session logout
  or token list; rotation via `clear()` is all-or-nothing.
- No rate limiting exists on session-token *guessing* the way there is on
  the access-code POST — a brute-force attempt against `is_valid` isn't
  gated by `RateLimiter`. Given 165 bits of token entropy this is not
  practically exploitable, so no fix is proposed, but it's a structural gap
  worth naming (the rate limiter's protection is scoped to `/auth`, not to
  cookie-bearing requests generally).

## 6. Control-frame size caps (WebSocket transport)

**What's implemented:** `crates/rylus-transport/src/websocket.rs` defines
`MAX_TEXT_FRAME_SIZE: usize = 64 * 1024` (64 KB, `websocket.rs:19-21`),
documented in-line as applying to "control messages" (text frames). In the
read loop, any incoming `Message::Text` frame whose length exceeds
`MAX_TEXT_FRAME_SIZE` is logged with `warn!` and dropped via `continue`
(does not close the connection, just discards that one message)
(`websocket.rs:191-198`). `Message::Binary` frames — used for video — are
explicitly **not** size-checked: the match arm at `websocket.rs:208`
(`Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_)
=> {}`) does nothing with binary payload size at all, and the doc comment at
`websocket.rs:20` states this plainly: "Binary frames (video) are not
limited."

Related but distinct caps in the same file: `CHANNEL_BUFFER_SIZE = 32`
(`websocket.rs:29-31`, the internal mpsc channel depth for control messages,
providing backpressure) and `VIDEO_QUEUE_CAPACITY = 4`
(`websocket.rs:33-36`, a drop-oldest ring buffer for outbound video frames —
this bounds *queued* frame count for outbound video, not inbound frame
*size*).

**Why:** Capping text-frame size defends the JSON control-message parser
(`serde_json::from_str`, `websocket.rs:199`) against a malicious or buggy
client sending an oversized text payload to exhaust memory or CPU on
parsing; dropping rather than closing the connection tolerates a one-off
oversized frame without tearing down an otherwise-good session.

**Gap, stated honestly:** There is no cap on inbound binary frame size.
Since this server only accepts binary frames from clients incidentally
(the current protocol direction sends video server→client as binary,
per `Message::Binary` in the write loop at `websocket.rs:224`) the practical
exposure depends on whether the client→server direction ever sends binary
frames; regardless, the receive loop's `Message::Binary(_) => {}` branch
would accept an arbitrarily large binary frame from any authenticated,
Origin-checked peer with no size limit at the transport layer before it
even reaches application logic. This is a real, unmitigated gap — not just
a theoretical one — and should be considered for a future todo (e.g. a
`MAX_BINARY_FRAME_SIZE` cap mirroring the text-frame one, or relying on
tokio-tungstenite's own frame-size limits if configured, which this code
does not currently set via `WebSocketStream::from_raw_socket`'s `None`
config argument at `websocket.rs:148-152`).

## Summary of the honest weak points found

- No cap on inbound binary WebSocket frame size (§6) — the most concrete gap.
- `Auto` TLS mode persists an unhardened private key under world-readable
  `/tmp/rylus/` (§4).
- Rate limiting is per-source-IP with in-memory-only state, so it's
  NAT-blind and doesn't survive a restart (§3).
- Session cookie has no `Secure` attribute, currently inert only because the
  server never runs a mixed HTTP+HTTPS listener (§5).
- `Disabled` TLS mode is accepted with only a warning log, and the default
  bind address is `0.0.0.0`, so a misconfigured deployment is
  LAN-cleartext-reachable rather than localhost-only (§4).
