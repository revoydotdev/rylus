# Changelog

## [Unreleased] — 1.0 path

### Added

- **`rylus-server --self-test` flag:** boots the server, captures one frame
  (synthetic source, no display required), encodes one software H.264 frame,
  accepts one WebSocket client (`101 Switching Protocols`), then exits `0`/`1`
  under a watchdog timeout. Wired into a per-OS CI smoke matrix that gates the
  release builds via `needs:`.
- **`docs/PROTOCOL.md` wire-format spec:** documents all protocol messages
  (Hello/HelloAck/HelloNack, Config, CapturableList, Heartbeat/HeartbeatAck,
  RequestKeyframe, ClientRtt, NewVideo/VideoFrame), framing (little-endian
  length prefix + JSON/binary payload), and versioning contract (protocol
  version 3 as of v0.17.0, `MIN_CLIENT_PROTOCOL_VERSION` enforced via
  `HelloNack`).
- **`docs/SECURITY-REVIEW.md`:** pre-1.0 security review covering argon2 auth
  params, rate-limit tuning, TLS cert generation and storage, session-token
  lifecycle, and explicit WebSocket `Origin` validation.
- **README accuracy pass:** corrected stale WebSocket port 9001 references
  (WS is now at `/ws` on the same port as HTTP), corrected TLS default (Auto
  mode generates a self-signed cert on first run, not "no TLS by default"),
  documented `--tls-mode` / `--tls-cert-path` / `--tls-key-path` flags, added
  minimal headless invocation examples, and updated badge/clone URLs to the
  `Chorosyne/rylus` org.
- **Latency budget benchmark harness (`rylus-encode` `bench` bin):** a
  std-only harness that times the display-free hot paths — software H.264
  encode of a synthetic frame (the `--self-test` path) and protocol message
  serialize/deserialize — against explicit per-target latency budgets, exiting
  non-zero when a budget is exceeded. Wired into a `bench.yml` CI job so a
  latency regression fails the build.

### Fixed

- **TLS auto-mode private key is no longer world-readable.** The self-signed
  cert/key pair is now stored under the per-user XDG state dir
  (`~/.local/state/rylus`, falling back to `~/.config/rylus`) instead of the
  shared `/tmp/rylus`. On Unix the directory is created `0700` and the private
  key is written `0600`; the public cert remains `0644`.

## [Unreleased]

### Added

- **AUR packaging:** source-based `rylus` and prebuilt `rylus-bin` PKGBUILDs
  under `packaging/aur/`, with matching `.SRCINFO`. A new `publish-aur` CI
  job pushes both on tag releases (gated on the `AUR_SSH_KEY` secret).
- **`--print-man-page` flag:** emits the roff-formatted man page to stdout
  via `clap_mangen`, so packagers can install `rylus.1` without a separate
  build-time dependency tree.
- **Icon asset:** first real `packaging/icons/rylus.svg` plus a raster
  generator (`scripts/gen-icons.sh`). Desktop entry now references `rylus`
  instead of the generic `input-tablet` fallback.
- **Systemd user unit** (`packaging/systemd/rylus.service`) for headless
  autostart on Linux.

## [0.17.0] - 2026-04-18

### Added

- **TLS-by-default:** Server now generates and serves a self-signed
  certificate on first run unless `--tls-mode disabled` is passed. Access
  codes and input events no longer travel in cleartext on the LAN by
  default. A loud `WARN` is logged when TLS is explicitly disabled.
- **mDNS service discovery:** Server publishes `_rylus._tcp.local.` with
  a short PID-derived collision suffix, so two Rylus instances on the
  same LAN never silently clobber each other's instance name.
- **Multi-device broadcast scaffolding (internal):** `StreamSession` with
  `tokio::sync::broadcast` fan-out (capacity 32) lands as internal plumbing
  for a future multi-tablet mode. Production still runs per-client capture
  + encode — connecting a second tablet today doubles CPU/GPU load.
  Tracked for a future release.
- **Connection-quality indicator:** Tablet UI exposes a corner pip
  (green/amber/red) driven by RTT derived from server `HeartbeatAck`
  echoes every 5 seconds. Hover/tap for numeric RTT and jitter.
- **Client-side palm rejection:** Pen contact suppresses incoming touch
  events for 100 ms, gated by a new **Palm Rejection** settings toggle.
  Only active when stylus input is enabled.
- **Stylus pressure curves:** User-selectable `linear` / `soft` / `firm`
  presets applied to pen pressure before it ships on the wire. Persisted
  in `localStorage`.
- **ABS_DISTANCE hover:** Linux uinput stylus declares the distance axis
  at device setup and emits a heuristic hover distance (full on 0 pressure,
  zero otherwise) with a 50 ms lift-off timeout workaround for browsers
  that don't fire `pointerleave` consistently.
- **PWA manifest + service worker:** Tablet page can now be installed to
  the home screen and survives brief network blips by caching the shell
  (index, `lib.js`, `style.css`, manifest). The SW never intercepts `/ws`
  or `/api/*` so streaming state stays live.
- **HelloNack + protocol versioning guard:** Server rejects clients below
  `MIN_CLIENT_PROTOCOL_VERSION` with a typed `HelloNack` message; the
  client reacts by toasting "page is out of date" and reloading after 2 s.
- **Chrome-on-iPadOS detection:** One-time toast steers users to Safari
  or Firefox (which work) instead of failing silently in MSE playback.

### Changed

- **Design refresh:** `www/static/style.css` rewritten to match
  DESIGN.md — `#1e1e1e`/`#2a2a2a` surfaces, Geist font stack with
  system fallback, toast component, focus-visible ring, and
  `prefers-reduced-motion` honoring motion transitions.
- **Error UI:** All three `alert()` calls replaced with inline toasts
  (error variant). Per DESIGN.md: inline contextual errors, never modal
  dialogs.
- **Heartbeat interval:** Reduced from 30 s to 5 s so RTT samples
  refresh at human timescale for the connection-quality pip, still well
  within the 60 s idle timeout.
- **Client handshake:** Declares `protocol_version: 3` instead of `2`.

### Fixed

- **Auth bypass on `/settings` and `/api/config`:** The server settings
  page and its JSON config API are now gated behind the same access-code
  session cookie as `/stream`. Previously an unauthenticated LAN peer
  could enumerate capturables, read the access code, or overwrite the
  server config.

### Dev / CI

- **Tests in CI:** Quality gate now runs `cargo test --workspace` and
  `npm test` (84 TS unit tests + 210 Rust unit tests) before any build.
- **Clippy warnings cleared:** Resolved `let_unit_value`,
  `large_enum_variant`, and `duplicated_attribute` warnings so the
  `-D warnings` gate doesn't need clippy suppressions outside TLS
  stream-enum hot-path indirection.
- **Esbuild bundling:** `package.json` and `build.rs` switched to
  `esbuild --bundle` so `ts/lib.ts` can `import` from `ts/utils.ts`.
  New `ts/sw.ts` bundle produces `www/static/sw.js`.

## [0.16.0] - 2026-03-24

### Added

- **Portal session caching:** PipeWire capturables now share a single portal
  session via a global `Weak<PortalSession>` cache, eliminating redundant
  `CreateSession` D-Bus calls and compositor prompts
- **Portal session RAII:** `PortalSession` calls
  `org.freedesktop.portal.Session.Close()` on drop, preventing leaked
  compositor sessions
- **Capture failure auto-teardown:** Video loop tears down the recorder
  and notifies the client after 30 consecutive capture failures instead of
  spinning indefinitely

### Fixed

- **DMA-BUF/MemFd SEGV:** Removed `MAP_BUFFERS` flag and manually mmap
  DMA-BUF and MemFd buffers with proper `DMA_BUF_IOCTL_SYNC`, preventing
  segfaults from PipeWire's broken auto-mmap on DMA-BUF file descriptors
- **Encoder buffer validation:** Added bounds checks for BGR0, BGR0S, RGB,
  and RGB0 pixel buffers before writing raw pointers into FFmpeg's AVFrame
- **PipeWire stream error detection:** Stream Error/Unconnected states are
  now tracked and surfaced from `capture()` instead of silently spinning
- **PipeWireRecorder drop order:** Fields reordered so listener drops before
  stream before mainloop, preventing use-after-free during teardown

## [0.15.1] - 2026-03-20

### Fixed

- **FFmpeg 8+ compatibility:** Migrated buffersink pixel format from deprecated
  `pix_fmts` binary option to new `pixel_formats` string API, with automatic
  fallback for FFmpeg < 7.1
- **WebSocket idle disconnect loop:** Added client heartbeat (30s interval) to
  keep connections alive during PipeWire capture timeouts. New `Heartbeat`
  protocol message; bumped protocol version to 2
- **MSE InvalidStateError cascade:** Guarded `sourceBuffer.buffered` access with
  `readyState === "open"` check and try-catch. Added inner try-catch in upd_buf
  error recovery. Debounced `sourceBuffer.onerror` restart (5s) to prevent
  tight NewVideo loop on persistent decode errors

### Changed

- **Build pipeline:** Replaced `tsc` with `esbuild` in build.rs. Added
  `rerun-if-changed` for both TypeScript source and generated JS to ensure
  `cargo build` picks up client changes
- **Default features:** GUI feature enabled by default for local development

## [0.15.0] - 2026-03-19

### Changed

- **Pure Rust X11 capture:** Replaced 785 lines of C code (xcapture.c,
  xhelper.c) with x11rb crate. Eliminated rylus-ffi crate and 6 system
  library link deps. Zero custom C code remains in the project.
- **Direct PipeWire capture:** Replaced GStreamer dependency chain (~30
  transitive C libraries) with direct pipewire-rs bindings for Wayland
  screen capture
- **Official Windows bindings:** Migrated from community winapi crate to
  Microsoft's official windows crate with safe COM wrappers and Result
  error handling
- **Pure Rust GUI:** Replaced fltk-rs (C++ FLTK bindings) with egui/eframe.
  New hero action layout, custom Rylus dark/light theme per DESIGN.md,
  QR code on all platforms, inline contextual errors, first-run hints

### Removed

- `rylus-ffi` crate (C FFI compilation layer)
- All C source files: xcapture.c, xhelper.c, xhelper.h, error.h, log.h
- `gstreamer`, `gstreamer-app`, `gstreamer-video` dependencies
- `winapi`, `wio` dependencies
- `fltk` dependency and 8-theme picker

## [0.14.0] - 2026-03-19

### Added

- WebSocket auto-reconnect in web client — exponential backoff (1s–30s),
  countdown banner with "Retry Now" button, 10 max attempts, full state
  preservation across reconnections
- Frame drop metrics logged at INFO level every 5 seconds for pipeline
  health visibility (capture drops and pacing drops)
- Comprehensive test suite: 97 tests across rylus-core (protocol serde,
  config parsing, pixel formats, error handling) and rylus-server (access
  code auth, rate limiting, session tokens)
- DESIGN.md design system document defining color tokens, typography,
  spacing, and component patterns for both native and web clients
- CI quality gates: clippy, rustfmt, and cargo-audit run before builds

### Fixed

- Geometry matching panic in enigo_device.rs on Linux — VirtualScreen
  variant now handled instead of crashing
- All 54 `unwrap()` calls replaced with proper error handling or
  descriptive `.expect()` messages

### Changed

- Moved Capturable, Recorder, Geometry traits from rylus-capture to
  rylus-core (rylus-capture re-exports for backwards compatibility)
- SAFETY comments added to all ~60 unsafe blocks documenting invariants

## [0.13.0] - 2026-03-19

### Security

- Access codes are now hashed with argon2 (constant-time verification)
  instead of plain-text string comparison
- Authentication moved from GET query parameters to POST form body —
  access codes no longer appear in URLs, server logs, or browser history
- Authenticated sessions use HttpOnly SameSite=Strict cookies
- Per-IP rate limiting on failed authentication attempts (5 attempts per
  60s window, 30s lockout)
- WebSocket text frames limited to 64KB to prevent OOM from oversized
  control messages
- 60s idle timeout closes zombie WebSocket connections, freeing video
  thread, encode thread, and input device resources

### Removed

- Dead C source files (encode_video.c, uinput.c) that were replaced by
  pure Rust implementations in v0.12.0 but remained in the repository

## [0.12.1] - 2026-03-16

### Fixed

- D-Bus portal response race condition in PipeWire screen capture — signal
  handlers are now registered before making method calls, preventing missed
  responses
- Wayland support flag is now auto-detected at runtime instead of being
  persisted to the config file, fixing incorrect behavior when switching
  between X11 and Wayland sessions

### Improved

- Show actionable "click Refresh List" guidance when no capturable is selected
  on Wayland instead of a generic error
- Frontend skips sending config when no capturables are available and
  auto-sends once capturables are populated after portal grant

## [0.12.0] - 2026-03-15

- Web settings UI and frontend improvements
- Refactored monolith into 8-crate Cargo workspace
- Updated build scripts, packaging, and documentation
