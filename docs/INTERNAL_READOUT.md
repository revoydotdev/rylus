# Internal Readout: Rylus Workspace Refactoring

## Repository Overview

- **Original:** H-M-H/Weylus, 711 commits across all branches
- **Fork divergence:** Local uncommitted refactoring (0 fork commits in git history)
- **Version:** 0.11.4 (last tagged) -> 0.12.0 (workspace WIP)

## Architecture Change

- **Before:** Single crate, monolithic `src/` (6,699 lines Rust, 24 files)
- **After:** Cargo workspace, 8 crates in `crates/` (7,015 lines Rust, 31 files)
- **Delta:** +316 lines (+4.7%), +7 files -- increase from workspace boilerplate (`lib.rs`, `Cargo.toml` per crate)

## Code Composition (New Workspace)

| Crate | Lines | Purpose |
|-------|-------|---------|
| rylus-capture | 2,218 | Screen capture (X11, PipeWire, macOS, Windows) |
| rylus-input | 1,618 | Input devices (uinput, autopilot, Windows) |
| rylus-server | 1,209 | HTTP server, session management, main binary |
| rylus-gui | 813 | FLTK desktop GUI |
| rylus-core | 540 | Config, protocol, error types, pixel types |
| rylus-ffi | 322 | FFI bindings, C compilation, FFmpeg linkage |
| rylus-transport | 161 | WebSocket transport layer |
| rylus-encode | 134 | Video encoder wrapper |
| **Total Rust** | **7,015** | |

## Non-Rust Code

| Type | Lines | Files |
|------|-------|-------|
| C/C++ (lib/) | 2,205 | 9 files (FFI: video encoding, X11 capture, uinput) |
| TypeScript (ts/) | 1,160 | 1 file (web client) |
| HTML/CSS (www/) | 330 | 3 files (templates + static) |

## Legacy vs New Assessment

- 100% of Rust source has been refactored (monolith -> workspace)
- 0% of Rust source is wholly new functionality -- this is a structural refactoring, not a feature rewrite
- C FFI code (`lib/`), TypeScript (`ts/`), and web assets (`www/`) are carried forward with quality improvements applied
- Build infrastructure (`build.rs`) fully migrated to per-crate build scripts
- Quality improvements applied: unwrap cleanup, memory leak fixes (Windows), SAFETY comments, TypeScript bug fixes, HTML5 validation

## What Remains from Original Weylus

- Core algorithms: screen capture pipeline, input injection
- Protocol definition: same wire protocol
- Web client: same TypeScript client with bug fixes
- C FFI layer: X11 capture helpers (xcapture.c/xhelper.c) still in C
- Video encoding: rewritten in Rust using ffmpeg-sys-next (system FFmpeg)
- Linux input: rewritten in Rust using evdev crate
- macOS/Windows input: rewritten using enigo crate (replaced autopilot fork)
- Build: system FFmpeg required (no more source build in `deps/`)

## What's New in the Refactoring

- Cargo workspace architecture (8 modular crates)
- `rylus-core` crate extracting shared types
- `rylus-transport` crate isolating WebSocket layer
- `session.rs` extracted from monolithic `websocket.rs`
- Per-crate build scripts replacing monolithic root `build.rs`
- Systematic quality pass: safer error handling, memory fixes, HTML5 compliance
