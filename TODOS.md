# TODOS

## Security

## Rust-Only Migration


### Rewrite X11 capture in pure Rust (x11rb)

**What:** Replace `lib/linux/xcapture.c` (280 lines) + `lib/linux/xhelper.c` (505 lines) with pure Rust using the `x11rb` crate. Eliminate the entire `rylus-ffi` crate.

**Why:** This is the last custom C code in the project. Rewriting eliminates 6 system library link deps (X11, Xext, Xrandr, Xfixes, Xcomposite, Xi), the `cc` build dependency, and the `rylus-ffi` crate. Moves the project to zero custom C code. x11rb provides a pure Rust X11 protocol implementation with better error handling and async support.

**Context:** Current C code handles: (1) XShm screen capture (xcapture.c), (2) window/monitor enumeration via XRandr (xhelper.c), (3) cursor compositing via XFixes, (4) input device mapping via XInput2. x11rb has extensions for all of these: `xshm`, `randr`, `xfixes`, `xinput`. The Rust wrapper in `crates/rylus-capture/src/x11.rs` (343 lines) defines the safe Rust interface — this becomes the implementation file instead of an FFI wrapper. The `DisplayGuard` RAII pattern translates directly to x11rb's connection model. After migration, delete `lib/linux/xcapture.c`, `lib/linux/xhelper.c`, `lib/linux/xhelper.h`, `lib/error.h`, `lib/log.h`, and the entire `crates/rylus-ffi/` directory.

**Effort:** L
**Priority:** P1
**Depends on:** Delete dead C files

### Migrate Windows bindings from winapi to windows crate

**What:** Replace `winapi` v0.3.9 with Microsoft's official `windows` crate for all Windows API calls (D3D11, DXGI, input).

**Why:** `winapi` is community-maintained, unsafe-heavy, and no longer actively developed. The `windows` crate is Microsoft's official Rust bindings with safe COM wrappers, active maintenance, and better ergonomics. Mechanical migration that improves safety.

**Context:** Windows-specific code lives in `crates/rylus-capture/src/captrs_capture.rs` (66 lines), `crates/rylus-capture/src/win_ctx.rs` (104 lines), and Windows-specific sections of `rylus-input`. The `captrs` crate (screen capture) may also need updating or replacement if it depends on `winapi` internally. Migration is mostly import path changes: `winapi::um::d3d11` → `windows::Win32::Graphics::Direct3D11`.

**Effort:** M
**Priority:** P2
**Depends on:** None

### Replace GStreamer with pipewire-rs for Wayland capture

**What:** Replace the `gstreamer`/`gstreamer-app`/`gstreamer-video` dependency chain with direct `pipewire-rs` bindings for PipeWire screen capture on Wayland.

**Why:** GStreamer is a heavy C dependency (~30 transitive C libraries). `pipewire-rs` talks directly to the PipeWire daemon, eliminating the GStreamer middle layer. Still links to C libpipewire but massively reduces the dependency surface. Simpler pipeline with less abstraction.

**Context:** PipeWire capture lives in `crates/rylus-capture/src/pipewire.rs` (704 lines). Current flow: D-Bus portal → GStreamer pipeline → AppSink → raw frames. New flow: D-Bus portal → pipewire-rs stream → raw frames. The D-Bus portal negotiation (`xdg-desktop-portal`) stays the same (already uses the `dbus` Rust crate). Format negotiation and pixel format conversion currently handled by GStreamer would move to the Rust side (or FFmpeg via rylus-encode). The zero-copy DMA-BUF TODO becomes easier with direct PipeWire access.

**Effort:** L
**Priority:** P2
**Depends on:** None

### Migrate GUI from FLTK to egui

**What:** Replace `fltk-rs` (C++ FLTK bindings) with `egui` (pure Rust immediate-mode GUI) for the native desktop interface. Redesign the layout with intentional information hierarchy and modern UX patterns.

**Why:** fltk-rs compiles the C++ FLTK library, adding ~30-50s to build time and requiring a C++ compiler. egui is pure Rust with no C/C++ compilation, modern immediate-mode API, and excellent cross-platform support. The Rylus GUI is simple (settings panel, QR code, connection status) — perfect for egui. This is also the opportunity to fix the flat, unhierarchical layout.

**Context:** GUI code lives in `crates/rylus-gui/src/lib.rs` (813 lines). Use `eframe` as the app framework (handles windowing). The GUI is already optional (feature flag `gui`), so this is a drop-in replacement behind the same flag.

**Design Decisions (from design review 2026-03-19):**

1. **Hero action layout:** Start/Stop button and connection URL + QR code at top as the primary visual element. Settings grouped in collapsible sections below (Connection, Encoding, Preferences). Log viewer collapsible at bottom.

2. **Custom Rylus theme:** Single intentional theme matching web client — dark/light mode following system preference (`prefers-color-scheme` equivalent), cyan accent (`#00aaff`), dark bg `#303030`, light bg `#eee`. Drop the 8-theme picker.

3. **Inline contextual errors:** Replace FLTK `alert()` dialogs with inline error messages below the relevant input field (e.g., red text "Not a valid IP address" under bind address). Non-blocking, contextual.

4. **QR code on all platforms:** Enable QR code display on macOS and Windows (currently Linux-only). The FLTK image handling limitation is gone with egui's native texture support.

5. **First-run experience:** On first launch (no saved config), show contextual hint text: "Start the server, then scan the QR code on your tablet to connect." Below collapsible sections: "(defaults work for most setups)". Disappears after first successful server start.

6. **Interaction states:** Button shows "Starting..." with spinner during server start. Server start failure shows inline error below button (not alert dialog). Empty log viewer shows "No log messages yet. Start the server to begin."

**Effort:** L
**Priority:** P3
**Depends on:** Create DESIGN.md (soft dependency)

## UX

### Add tablet-optimized bottom sheet settings panel

**What:** On tablet viewports (480px–1024px), replace the side-drawer settings panel with a draggable bottom sheet. Half-height by default, drag handle to expand to full-height. Tab navigation across settings groups (Capture | Video | Input | Display).

**Why:** The settings side-drawer covers ~25% of the video on a 10" tablet in landscape. Tablets are the primary device class for Rylus's web client. A bottom sheet is a well-understood mobile/tablet pattern that doesn't occlude the video horizontally.

**Context:** Settings panel CSS is in `www/static/style.css` (`.settings` class, 16em wide). JS toggle logic in `ts/lib.ts`. Add a `@media (min-width: 480px) and (max-width: 1024px)` breakpoint. Bottom sheet needs: drag handle element, CSS `transform: translateY()` for slide-up, touch drag handler for resize, tab bar for section navigation. Lefty mode not applicable to bottom sheet (already centered).

**Effort:** M
**Priority:** P2
**Depends on:** None

## Architecture

## Code Quality

## CI/CD

## Testing

## Performance

### Investigate PipeWire zero-copy via DMA-BUF

**What:** Eliminate frame memcpy in PipeWire capture→encode path using FFmpeg DMA-BUF/hwframe import.

**Why:** At 1080p@30fps, copying each frame from PipeWire buffer to Rust Vec is ~186MB/s of unnecessary memcpy. Zero-copy would reduce CPU usage and latency.

**Context:** The existing TODO in `crates/rylus-capture/src/pipewire.rs` notes this. FFmpeg supports `AV_HWDEVICE_TYPE_DRM` / `AV_PIX_FMT_DRM_PRIME` for DMA-BUF import. PipeWire can expose DMA-BUF file descriptors for captured frames. The challenge is connecting PipeWire's buffer → FFmpeg's hwframe without the intermediate copy. This touches unsafe FFI in `rylus-encode` and requires testing across GPU vendors (Intel, AMD, NVIDIA). Profile the pipeline first to confirm memcpy is actually the bottleneck vs encoder or network.

**Effort:** XL
**Priority:** P3
**Depends on:** Add frame drop metrics and pipeline timing

## Completed

### Delete dead C files (encode_video.c, uinput.c)

**What:** Remove `lib/encode_video.c` (933 lines) and `lib/linux/uinput.c` (315 lines) from the repository.

**Why:** Both files were replaced by pure Rust implementations (rylus-encode crate and evdev crate respectively) but still exist in the repo.

**Effort:** S
**Priority:** P0
**Completed:** v0.12.2 (2026-03-19)

### Harden access code authentication

**What:** Hash access codes with argon2, add rate limiting on failed attempts, move code from query params to POST body.

**Why:** Current plain-text string comparison was timing-attack vulnerable. Access codes appeared in server logs, browser history, and URL bars.

**Effort:** M
**Priority:** P0
**Completed:** v0.12.2 (2026-03-19)

### Add WebSocket frame size limits and connection idle timeout

**What:** Set max text frame size (64KB). Add 60s idle timeout that closes WebSocket if no messages received.

**Why:** No frame size limit meant a malicious client could OOM the server. No idle timeout meant zombie connections leaked resources.

**Effort:** S
**Priority:** P0
**Completed:** v0.12.2 (2026-03-19)

### Create DESIGN.md

**What:** Formal design system document for both native egui client and web tablet client.

**Effort:** S
**Priority:** P1
**Completed:** v0.13.1 (2026-03-19)

### Add clippy, rustfmt, and cargo-audit to CI pipeline

**What:** CI quality gates running before build jobs.

**Effort:** S
**Priority:** P1
**Completed:** v0.13.1 (2026-03-19)

### Add SAFETY comments to all unsafe blocks

**What:** Document invariants and soundness reasoning for all unsafe blocks across the codebase.

**Effort:** M
**Priority:** P1
**Completed:** v0.13.1 (2026-03-19)

### Audit and fix all unwrap() calls

**What:** Replace all unwrap() calls with proper error handling. Fixed geometry matching bug in enigo_device.rs.

**Effort:** M
**Priority:** P1
**Completed:** v0.13.1 (2026-03-19)

### Write comprehensive test suite

**What:** 97 unit tests across rylus-core (74) and rylus-server (23) covering protocol serde, config parsing, pixel types, error handling, access code auth, rate limiting, and session management.

**Effort:** L
**Priority:** P1
**Completed:** v0.14.0 (2026-03-19)

### Move Capturable trait + Geometry to rylus-core

**What:** Moved Capturable, Recorder, Geometry, and BoxCloneCapturable from rylus-capture to rylus-core.

**Effort:** S
**Priority:** P2
**Completed:** v0.14.0 (2026-03-19)

### Add frame drop metrics and pipeline timing

**What:** Frame drop counter with periodic INFO logging every 5 seconds.

**Effort:** S
**Priority:** P2
**Completed:** v0.14.0 (2026-03-19)

### Add auto-reconnect to web client

**What:** WebSocket auto-reconnect with exponential backoff, countdown display, and state preservation.

**Effort:** M
**Priority:** P1
**Completed:** v0.14.0 (2026-03-19)
