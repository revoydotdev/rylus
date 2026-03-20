# TODOS

## Security

## Rust-Only Migration

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

### Investigate H.264 profile/level mismatch between encoder and MSE codec string

**What:** The MSE codec string `avc1.4D403D` declares Main Profile Level 6.1, but nvenc may produce High Profile by default. A mismatch causes silent MSE decode errors and wasted `onerror` restarts (now debounced but still wasteful).

**Why:** If the encoder outputs High Profile (0x64) but the browser expects Main Profile (0x4D), frames decode intermittently or fail entirely depending on browser strictness. This may be the root cause of the "one or two flashes then black" symptom.

**Context:** Codec string is hardcoded in `ts/lib.ts` line ~1000. nvenc codec setup is in `crates/rylus-encode/src/lib.rs` `try_nvenc()` — no explicit `profile` is set. Fix either by setting `-profile:v main` on the encoder, or by reading the actual profile/level from the fMP4 init segment and generating the codec string dynamically (e.g., `avc1.640033` for High Profile Level 5.1).

**Effort:** M
**Priority:** P1
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

### Rewrite X11 capture in pure Rust (x11rb)

**What:** Replaced 785 lines of C (xcapture.c, xhelper.c) with pure Rust x11rb. Eliminated rylus-ffi crate and 6 system library link deps.

**Effort:** L
**Priority:** P1
**Completed:** v0.15.0 (2026-03-19)

### Replace GStreamer with pipewire-rs for Wayland capture

**What:** Direct pipewire-rs stream replaces gstreamer/gstreamer-app/gstreamer-video dependency chain.

**Effort:** L
**Priority:** P2
**Completed:** v0.15.0 (2026-03-19)

### Migrate Windows bindings from winapi to windows crate

**What:** Replaced winapi with Microsoft's official windows crate. Safe COM wrappers, Result error handling.

**Effort:** M
**Priority:** P2
**Completed:** v0.15.0 (2026-03-19)

### Migrate GUI from FLTK to egui

**What:** Pure Rust egui/eframe GUI with hero action layout, custom Rylus theme, QR code on all platforms, inline errors, first-run hints.

**Effort:** L
**Priority:** P3
**Completed:** v0.15.0 (2026-03-19)
