# Changelog

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
