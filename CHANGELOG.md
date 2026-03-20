# Changelog

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
