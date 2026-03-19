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

**What:** Replace `fltk-rs` (C++ FLTK bindings) with `egui` (pure Rust immediate-mode GUI) for the native desktop interface.

**Why:** fltk-rs compiles the C++ FLTK library, adding ~30-50s to build time and requiring a C++ compiler. egui is pure Rust with no C/C++ compilation, modern immediate-mode API, and excellent cross-platform support. The Rylus GUI is simple (settings panel, QR code, connection status) — perfect for egui.

**Context:** GUI code lives in `crates/rylus-gui/src/lib.rs` (813 lines). Key UI elements: bind address/port inputs, access code input, start/stop button, capturable list, connection status, QR code display, theme selection. egui provides all of these natively. Use `eframe` as the app framework (handles windowing). QR code rendering can use the existing `qrcode` crate output rendered to egui's painter. The 8 FLTK themes map to egui's `Visuals` system. The GUI is already optional (feature flag `gui`), so this is a drop-in replacement behind the same flag.

**Effort:** L
**Priority:** P3
**Depends on:** None

## Architecture

### Move Capturable trait + Geometry to rylus-core

**What:** Move `Capturable` trait, `Recorder` trait, `Geometry` enum, and related types from `rylus-capture` to `rylus-core`.

**Why:** Currently `rylus-input` depends on `rylus-capture` solely for the `Capturable` trait's geometry types. This creates an awkward sibling dependency. These are shared protocol concepts that belong in core alongside `MessageInbound`/`MessageOutbound`.

**Context:** `Capturable` is defined in `crates/rylus-capture/src/lib.rs`. `Geometry` is used by both capture and input crates. Moving to core removes the `input→capture` dependency edge. The platform-specific implementations (`X11Capturable`, `PipeWireCapturable`, etc.) stay in `rylus-capture`. Only the trait definition and geometry types move. Update `Cargo.toml` for both `rylus-capture` and `rylus-input` to remove the cross-dependency.

**Effort:** S
**Priority:** P2
**Depends on:** None

## Code Quality

### Add SAFETY comments to all unsafe blocks

**What:** Document invariants and soundness reasoning for all 68 unsafe blocks across the codebase.

**Why:** Only 7 of 68 unsafe blocks have SAFETY comments. Undocumented unsafe code is a maintenance hazard — future contributors (or future you) can unknowingly break invariants. The FFmpeg FFI in `rylus-encode` (25+ blocks) and `unsafe impl Send` in `rylus-transport` are the highest risk.

**Context:** Main unsafe areas: `rylus-encode/src/lib.rs` (FFmpeg pointer ops, AVIO callbacks, filter graph), `rylus-capture/src/x11.rs` (X11 FFI), `rylus-capture/src/core_graphics.rs` (macOS FFI), `rylus-transport/src/websocket.rs` (Send impl). Follow Rust convention: `// SAFETY: <why this is sound>` immediately before each unsafe block. For Send impls, document which fields are actually thread-safe and why.

**Effort:** M
**Priority:** P1
**Depends on:** None

### Audit and fix all unwrap() calls

**What:** Replace all 59 `unwrap()` calls with proper error handling. Fix confirmed bug in `enigo_device.rs` geometry matching.

**Why:** `unwrap()` in production code is a crash waiting to happen. One confirmed bug: `enigo_device.rs:49` calls `.geometry().unwrap()` then pattern-matches only `Geometry::Relative` — on Linux, a `VirtualScreen` variant panics. `CString::new().unwrap()` in `rylus-encode` (4 occurrences) panics on strings with null bytes.

**Context:** Triage by severity: (1) `enigo_device.rs:49` geometry bug — add fallback arm, (2) `rylus-encode` CString calls — use `.expect("reason")` or propagate Result, (3) `rylus-gui` mutex locks — `.unwrap()` is acceptable for poisoned mutexes (unrecoverable), document with `.expect("mutex poisoned")`, (4) remaining sites — case-by-case. Run `rg 'unwrap\(\)' crates/` to find all sites.

**Effort:** M
**Priority:** P1
**Depends on:** None

## CI/CD

### Add clippy, rustfmt, and cargo-audit to CI pipeline

**What:** Add CI steps for `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`, and `cargo audit`.

**Why:** No code quality gates in CI. Builds succeed with warnings, dead code, formatting drift, or known CVEs in dependencies. These are the cheapest layer of the testing pyramid.

**Context:** CI is in `.github/workflows/build.yml`. Currently only builds and packages. Add a separate job (or early step) that runs clippy+fmt+audit before the build matrix. `cargo-audit` needs the `cargo-audit` binary installed. Consider using `actions-rs/audit-check` or installing via `cargo install cargo-audit`. Clippy and fmt are built into rustup.

**Effort:** S
**Priority:** P1
**Depends on:** None

## Testing

### Write comprehensive test suite

**What:** Unit tests for all pure/testable codepaths across all 8 crates. Integration test for TestSrc capture→encode pipeline.

**Why:** Zero test coverage. Every codepath is untested. Protocol serialization, config parsing, coordinate transforms, key mapping, quality adaptation logic, and HTTP routing are all pure functions that are trivially testable.

**Context:** Priority order: (1) `rylus-core` — protocol serde roundtrip, config TOML parsing, Geometry construction, CError Display, pixel format conversions, (2) `rylus-input` — key code mapping completeness, coordinate transforms, geometry matching edge cases, (3) `rylus-encode` — quality adaptation (QP calculation from buffer health/pipeline ratio), (4) `rylus-server/web` — HTTP routing, access code validation, template rendering, (5) `rylus-transport` — message parsing, (6) `rylus-capture` — TestSrc output, (7) Integration — TestSrc→encode pipeline end-to-end. Use `#[cfg(test)] mod tests` in each crate's lib.rs. The `testsrc.rs` capturable already exists as a synthetic test source.

**Effort:** L
**Priority:** P1
**Depends on:** None

## Performance

### Add frame drop metrics and pipeline timing

**What:** Track frame drop count/rate in the video capture thread. Log periodically at INFO level. Expose via protocol for frontend display.

**Why:** Frames are silently dropped via `try_send()` when the encoder can't keep up. Without visibility into drop rate, diagnosing "why does it feel laggy?" requires guesswork. The adaptive quality system adjusts QP based on buffer health, but there's no capture-side visibility.

**Context:** Frame drops happen in `crates/rylus-server/src/session.rs` in the `handle_video()` function where `try_send()` returns `Err`. Add a counter that increments on each drop and logs every 5 seconds (or N frames). Optionally add a new `MessageOutbound` variant for frame stats so the TypeScript frontend can display drop rate alongside FPS.

**Effort:** S
**Priority:** P2
**Depends on:** None

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
