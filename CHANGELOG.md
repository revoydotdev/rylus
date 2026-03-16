# Changelog

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
