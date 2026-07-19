# ADR-0002 — Pure-Rust modular workspace

- **Status:** Accepted
- **Date:** 2026-07-19
- **Relates to:** VISION.md AX-5, AX-8

## Context

Upstream Weylus was a single-crate project mixing Rust with ~2,200 lines of
custom C/C++ (X11 capture helpers, uinput, video encoding) and a chain of C
library dependencies: GStreamer and its ~30 transitive libraries for capture,
FLTK (via C++ bindings) for the GUI, and the community `winapi` crate for
Windows. Rylus exposes an HTTP and WebSocket service on the user's LAN, so the
whole codebase is part of the threat surface and must be auditable. A mixed
C/C++/Rust build also complicates the toolchain and spreads memory-safety
guarantees thin.

## Decision

Rylus is a pure-Rust Cargo workspace with no custom C or C++ code.

- **Screen capture:** the 785-line C X11 layer is replaced by `x11rb` (pure-Rust
  X11 protocol); the GStreamer chain is replaced by direct `pipewire-rs`
  bindings for Wayland.
- **GUI:** FLTK/`fltk-rs` is replaced by `egui`/`eframe` (immediate-mode, pure
  Rust).
- **Windows bindings:** the community `winapi` crate is replaced by Microsoft's
  official `windows` crate (safe COM wrappers, `Result` errors).
- **Structure:** the monolith is split into a seven-crate workspace with
  explicit boundaries — `rylus-core` (config, protocol, error, pixel formats,
  shared traits), `rylus-capture`, `rylus-encode`, `rylus-input`,
  `rylus-transport`, `rylus-gui`, `rylus-server`. Backends sit behind the
  `Capturable`/`Recorder`/`Geometry` traits defined in `rylus-core`.
- The one remaining foreign-code surface is FFmpeg via `ffmpeg-sys-next`, wrapped
  behind safe Rust in `rylus-encode`. Every `unsafe` block carries a `// SAFETY:`
  comment.

The `rylus-ffi` crate and all C source files were removed.

## Consequences

- A single Rust toolchain builds the whole project; memory-safety guarantees
  extend to every line of application code, satisfying AX-5.
- Crate boundaries make the capture/encode/input/transport concerns swappable
  and independently testable, and let per-platform backends satisfy AX-8 without
  cross-contamination.
- FFmpeg remains a system dependency and an FFI boundary — encoder buffer writes
  are bounds-checked, and that surface is the primary place `unsafe` remains.
- The dependency graph, error model, and build differ materially from upstream
  Weylus; the projects are no longer drop-in compatible at the source level,
  though the wire protocol and web client heritage are shared.
</content>
