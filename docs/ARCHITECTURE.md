# Architecture

Rylus is a pure-Rust Cargo workspace plus a TypeScript browser client. The
server captures the screen, encodes it to H.264/fMP4, and streams it over a
WebSocket to a browser-based tablet client, which sends back pointer, stylus,
and keyboard events that the server synthesizes as real input devices.

```
┌─────────────┐   PointerEvents / keys (WebSocket)   ┌──────────────────────┐
│  Tablet     │ ───────────────────────────────────► │  rylus-server        │
│  browser    │                                      │  (HTTP + WebSocket,  │
│  (ts/, www/)│ ◄─────────────────────────────────── │   session, auth)     │
└─────────────┘   H.264 / fMP4 video (WebSocket/MSE)  └──────────┬───────────┘
                                                                 │
                          ┌──────────────────────────────────────┼───────────────────┐
                          ▼                     ▼                 ▼                   ▼
                   rylus-capture        rylus-encode       rylus-input         rylus-transport
                   (screen frames)      (FFmpeg H.264)     (uinput/WinRT/...)   (WebSocket framing)
                          └──────────────── rylus-core (shared traits, config, protocol) ───────────┘
                                                   rylus-gui (egui desktop control panel)
```

## Workspace layout

Seven crates with explicit dependency boundaries. Backends are swappable behind
shared traits (`Capturable`, `Recorder`, `Geometry`) defined in `rylus-core`.

| Crate | Responsibility |
|-------|----------------|
| **rylus-core** | Config, wire protocol, error types, pixel formats, shared traits. No platform deps. |
| **rylus-capture** | Screen capture backends: X11 (x11rb + MIT-SHM), Wayland (pipewire-rs + xdg-desktop-portal), macOS (CoreGraphics), Windows (GDI). |
| **rylus-encode** | Video encoding via FFmpeg (`ffmpeg-sys-next`): H.264 in fragmented MP4, with VAAPI / NVENC / VideoToolbox / MediaFoundation hardware paths and a libx264 software fallback. |
| **rylus-input** | Input synthesis: Linux uinput/evdev (stylus pressure, tilt, multi-touch), Windows WinRT, macOS enigo. |
| **rylus-transport** | WebSocket transport — framing, 64 KB text-frame cap, 120 s idle timeout, optional TLS, heartbeat. |
| **rylus-gui** | Native desktop control panel (egui/eframe): QR code, theme, settings, inline errors. |
| **rylus-server** | HTTP + WebSocket server, session management, argon2 auth, rate limiting, mDNS, main binary. |

Non-Rust surface: `ts/` (TypeScript tablet client, built with esbuild), `www/`
(HTML templates + static assets), `packaging/` (AUR, systemd, desktop, icons).

## Data flow

1. **Capture** — `rylus-capture` grabs frames via the platform backend. X11 uses
   MIT-SHM shared memory; Wayland uses DMA-BUF/MemFd buffers synchronized with
   `DMA_BUF_IOCTL_SYNC` before access. Portal sessions are cached (`Weak<PortalSession>`)
   to avoid redundant D-Bus prompts.
2. **Encode** — frames are bounds-checked per pixel format (BGR0/BGR0S/RGB/RGB0),
   then encoded to H.264 and packaged as fragmented MP4 for browser Media Source
   Extensions. Keyframe-on-demand serves late-joining or recovering clients.
3. **Transport** — fMP4 segments stream to the client over WebSocket; pointer and
   key events flow back. Heartbeat (protocol v3) keeps the link alive through
   capture stalls and drives the client's RTT/quality indicator.
4. **Input** — the server routes events through the generic OS-independent backend
   (mouse only) or, on Linux, the uinput backend that creates virtual mouse,
   stylus, and multi-touch devices the OS treats as real hardware.

## Security model

Rylus exposes a network service on the LAN, so the threat surface is treated as
first-class (`rylus-server`, `rylus-transport`):

- argon2-hashed access codes, constant-time verification; codes sent in POST
  bodies (never URLs/logs/history).
- Per-IP rate limiting (5 failures / 60 s → 30 s lockout).
- HttpOnly, SameSite=Strict session cookies; token rotation.
- 64 KB control-frame cap; 120 s idle teardown.
- Optional self-signed TLS via rcgen (or an external proxy — see `rylus_tls.sh`).

Memory safety is language-guaranteed; the ~60 `unsafe` blocks (FFmpeg FFI,
platform capture) carry `// SAFETY:` invariants.

## Relationship to Weylus

Rylus is a fork of [H-M-H/Weylus](https://github.com/H-M-H/Weylus) that replaced
all C/C++ with Rust (the former `rylus-ffi`/C-FFI layer is gone), swapped
GStreamer for direct PipeWire bindings, moved the GUI from FLTK to egui, split
the monolith into this workspace, and added the security layer above. The wire
protocol and web-client concept are inherited; the dependency graph, build
toolchain, error model, and threat surface are materially different.
