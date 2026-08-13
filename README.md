# Rylus

[![Build](https://github.com/Chorosyne/rylus/actions/workflows/build.yml/badge.svg)](https://github.com/Chorosyne/rylus/actions/workflows/build.yml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021--edition-orange.svg)](https://www.rust-lang.org/)

**Turn a tablet or smartphone into a graphic tablet and touch screen for your computer — using only a browser.**

## About

Rylus lets you draw, write, and interact with your computer using a tablet or phone as an input device. Open a URL in your tablet's browser, and Rylus streams your screen while capturing stylus pressure, tilt, and multi-touch input. No app install required on the tablet side — any modern browser (Firefox 80+, Safari on iOS/iPadOS 13+) works.

### Origin and Fork Rationale

Rylus began as a fork of [H-M-H/Weylus](https://github.com/H-M-H/Weylus), which pioneered the idea of using browser [PointerEvents](https://developer.mozilla.org/en-US/docs/Web/API/PointerEvent) to capture stylus and touch input over a local network. That core design — a lightweight web client backed by a native capture and input server — remains the foundation Rylus builds on.

Over the course of development, the scope of changes grew large enough that the two projects now serve different goals. Rylus replaced all C and C++ code with pure Rust equivalents, swapped the GStreamer pipeline for direct PipeWire bindings, moved from FLTK to egui, restructured the codebase into a modular workspace, and added a security layer for network-exposed operation. These changes affect nearly every subsystem — the dependency graph, build toolchain, error handling model, and threat surface are all materially different from upstream.

Rylus prioritizes long-term maintainability, memory safety across the entire codebase, and production readiness for a tool that exposes a network service on your LAN. The upstream Weylus project deserves credit for the concept and initial implementation that made this work possible.

## What Changed from Weylus

### Pure Rust Architecture

All custom C and C++ code has been eliminated from the project:

- **X11 capture:** 785 lines of C (xcapture.c, xhelper.c, plus headers) replaced with [x11rb](https://github.com/psychon/x11rb), a pure Rust X11 protocol implementation. The `rylus-ffi` crate that compiled C FFI bindings was removed entirely.
- **Screen capture pipeline:** GStreamer and its ~30 transitive C library dependencies (glib, gobject, gstreamer core, plugins, etc.) replaced with direct [pipewire-rs](https://gitlab.freedesktop.org/pipewire/pipewire-rs) bindings for Wayland capture.
- **Desktop GUI:** FLTK (C++ bindings via fltk-rs) replaced with [egui/eframe](https://github.com/emilk/egui), a pure Rust immediate-mode GUI.
- **Windows platform bindings:** The community `winapi` crate replaced with Microsoft's official [windows](https://github.com/microsoft/windows-rs) crate, which provides safe COM wrappers and idiomatic `Result` error handling.

A single Rust toolchain now builds the entire project. Memory safety guarantees extend to every line of application code, making the codebase easier to audit and contribute to.

### Modular Workspace

The original monolithic structure has been refactored into a 7-crate Cargo workspace with explicit dependency boundaries:

| Crate | Purpose |
|-------|---------|
| **rylus-core** | Config, protocol definitions, error types, pixel formats, shared traits (`Capturable`, `Recorder`, `Geometry`) |
| **rylus-capture** | Screen capture backends (X11 via x11rb, PipeWire for Wayland, CoreGraphics, Windows GDI) |
| **rylus-encode** | Video encoding via FFmpeg (H.264, fMP4 container) |
| **rylus-input** | Input device backends (Linux uinput/evdev, Windows WinRT, macOS enigo) |
| **rylus-transport** | WebSocket transport layer |
| **rylus-gui** | Desktop GUI (egui/eframe) |
| **rylus-server** | HTTP/WebSocket server, session management, main binary |

Each crate compiles independently and backends are swappable behind shared traits defined in rylus-core.

### Security Hardening

Rylus exposes an HTTP and WebSocket service on your local network. The following measures address the basics for responsible deployment:

- **Access code storage:** Plaintext comparison replaced with argon2 hashing and constant-time verification.
- **Authentication transport:** Access codes moved from GET query parameters to POST request body — codes no longer appear in URLs, server logs, or browser history.
- **Session management:** Authenticated sessions use HttpOnly, SameSite=Strict cookies.
- **Rate limiting:** 5 failed authentication attempts per 60-second window triggers a 30-second lockout per IP.
- **WebSocket limits:** 64KB text frame maximum to prevent OOM from oversized control messages. 120-second idle timeout closes zombie connections, freeing video, encode, and input device resources.

### Reliability and Error Recovery

Real-world capture pipelines fail — hardware goes to sleep, compositors revoke portal access, network links drop. Rylus handles these cases with graceful degradation instead of panics or silent hangs:

- **WebSocket auto-reconnect:** Exponential backoff (1s to 30s), state preservation across reconnections, countdown banner with manual retry in the web client. 10 maximum attempts before giving up.
- **Heartbeat protocol:** 5-second keep-alive messages (protocol version 3) survive PipeWire capture timeouts that would otherwise look like idle connections, and drive the tablet's connection-quality indicator via RTT sampling.
- **Capture failure auto-teardown:** After 30 consecutive capture failures, the video loop tears down the recorder and notifies the client instead of spinning indefinitely.
- **MSE error recovery:** Debounced sourceBuffer restart (5-second cooldown), readyState guards to prevent InvalidStateError cascades on persistent decode errors.
- **Error handling cleanup:** 54 bare `unwrap()` calls replaced with proper error propagation or descriptive `.expect()` messages. SAFETY comments document invariants on all ~60 `unsafe` blocks.
- **PipeWire session management:** Portal session caching via `Weak<PortalSession>` eliminates redundant D-Bus calls and compositor prompts. RAII cleanup calls `Session.Close()` on drop. Explicit DMA-BUF and MemFd synchronization with `DMA_BUF_IOCTL_SYNC` prevents segfaults from unsynchronized buffer access.
- **Encoder buffer validation:** Bounds checks on BGR0, BGR0S, RGB, and RGB0 pixel buffers before writing raw pointers into FFmpeg's AVFrame.

### Developer Experience

- **Test suite:** 200+ tests across Rust and TypeScript covering protocol serialization, config parsing, pixel formats, access code authentication, rate limiting, session tokens, encoder buffer validation, and client reconnect logic.
- **CI quality gates:** clippy, rustfmt, and cargo-audit run before builds.
- **Structured logging:** The `tracing` crate with optional JSON output (`RYLUS_LOG_JSON=true`). Configurable log levels via `RYLUS_LOG_LEVEL`.
- **Frontend build:** esbuild replaces tsc for TypeScript compilation, with proper `rerun-if-changed` integration in build.rs.

### User Interface

- **Native GUI:** egui with custom dark/light Rylus theme, QR code display on all platforms, first-run hints, collapsible settings panel, and inline contextual error messages.
- **Web client:** Responsive settings panel, debug overlay, energy saving mode, reconnect countdown banner.

## Features

- Control your mouse with your tablet
- Mirror your screen to your tablet
- Send keyboard input using physical keyboards
- Hardware accelerated video encoding
- Pure Rust codebase — no custom C/C++ code
- Native desktop GUI built with [egui](https://github.com/emilk/egui)
- WebSocket auto-reconnect with exponential backoff
- Access code authentication with argon2 hashing and rate limiting

The above features are available on all operating systems. Additional features on Linux:

- Stylus/pen support with pressure and tilt
- Multi-touch input (works with apps like Krita that support multi-touch)
- Capturing specific windows and mapping input to them
- Faster screen mirroring via shared memory (X11) or DMA-BUF (Wayland)
- Tablet as second screen

## Platform Support

| Platform | Capture Backend | HW Accelerated Encoding |
|----------|----------------|------------------------|
| Linux (X11) | x11rb + MIT-SHM | VAAPI, NVENC |
| Linux (Wayland) | pipewire-rs + xdg-desktop-portal | VAAPI, NVENC |
| macOS | CoreGraphics | VideoToolbox |
| Windows | GDI via windows-rs | NVENC, MediaFoundation |

## Installation

Download the latest release for your OS from the [releases page](https://github.com/Chorosyne/rylus/releases). No apps are needed on your tablet — just a modern browser (Firefox 80+, Safari on iOS/iPadOS 13+).

**Arch Linux:** install from the AUR:

```
yay -S rylus-bin   # prebuilt binary
yay -S rylus       # source build
```

Both packages install a `rylus.service` user unit; `systemctl --user enable --now rylus` will start Rylus in headless mode on login.

**Linux users:** follow the [uinput setup instructions](#linux) to enable stylus pressure sensitivity and multi-touch support.

## Running

Start Rylus, optionally set an access code in the settings panel, and press Start. This launches a webserver on your computer. Open `http://<your computer's address>:<port, default 1701>` in a browser on your tablet. Rylus displays the URL and a QR code you can scan.

If you have a firewall, open TCP port 1701 (or whichever port you configured with `--web-port`). The WebSocket stream is served at `/ws` on the same port — no separate port is needed.

On many Linux distributions this is done with ufw:
```
sudo ufw allow 1701/tcp
```

Rylus supports access code authentication (hashed with argon2, rate-limited) and enables TLS by default (auto mode generates a self-signed certificate on first run). To disable TLS, pass `--tls-mode disabled`; to use a certificate authority-signed certificate, pass `--tls-mode certified` together with `--tls-cert-path` and `--tls-key-path`.

### Fullscreen

Add a bookmark to your home screen on your tablet to run Rylus in full screen mode (on iOS/iPadOS this must be done with Safari). On other platforms there is a button to toggle full screen mode.

### Keyboard Input

Rylus supports keyboard input for physical keyboards. Connect a Bluetooth keyboard to your tablet and start typing. Due to technical limitations, on-screen keyboards are not supported.

### Headless Mode

Rylus provides a command-line interface. `--no-gui` starts Rylus in headless mode, suitable for automation and remote servers.

Minimal headless invocation (starts immediately on port 1701 with TLS auto-enabled):

```sh
rylus --no-gui
```

With an access code and explicit TLS mode:

```sh
rylus --no-gui --access-code mysecret --web-port 1701 --tls-mode auto
```

To disable TLS (e.g. when terminating TLS at a reverse proxy):

```sh
rylus --no-gui --tls-mode disabled
```

For all options, run `rylus --help`.

Configuration is stored in `~/.config/rylus/rylus.toml`.

Logging is configurable via environment variables:
- `RYLUS_LOG_LEVEL` — set to `DEBUG` or `TRACE` for verbose output
- `RYLUS_LOG_JSON=true` — enable structured JSON log output

### Linux

Rylus uses the `uinput` interface to simulate input events on Linux. **To enable stylus and multi-touch support, `/dev/uinput` must be writable by Rylus.** To make `/dev/uinput` permanently writable by your user, run:

```sh
sudo groupadd -r uinput
sudo usermod -aG uinput $USER
echo 'KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"' \
| sudo tee /etc/udev/rules.d/60-rylus.rules
```

Then either reboot, or run:

```sh
sudo udevadm control --reload
sudo udevadm trigger
```

Then log out and log in again. To undo this:

```sh
sudo rm /etc/udev/rules.d/60-rylus.rules
```

This allows your user to synthesize input events system-wide, even when another user is logged in. Untrusted users should not be added to the uinput group.

#### Wayland

Rylus supports Wayland via direct PipeWire bindings and the xdg-desktop-portal. Install `pipewire` and `xdg-desktop-portal` along with the portal backend for your compositor:

- `xdg-desktop-portal-gtk` for GNOME
- `xdg-desktop-portal-kde` for KDE
- `xdg-desktop-portal-wlr` for wlroots-based compositors (Sway, etc.)

Known limitations on Wayland:
- Input mapping for individual windows is not supported
- Window names may not display correctly
- Cursor capture is not available

#### Hardware Acceleration

On Linux, Rylus supports hardware accelerated video encoding through VAAPI or Nvidia's NVENC. Hardware acceleration is disabled by default because quality varies across hardware — enable it in the settings if your hardware produces acceptable results.

**VAAPI configuration:**
- Select a specific driver by setting `LIBVA_DRIVER_NAME`. List available drivers with:
  ```sh
  ls /usr/lib/dri/ | sed -n 's/^\(\S*\)_drv_video.so$/\1/p'
  ```
- On some distributions, drivers reside in a different directory (e.g. `/usr/lib/x86_64-linux-gnu/dri`). Set `LIBVA_DRIVERS_PATH` to override the search path.
- Set `RYLUS_VAAPI_DEVICE` to specify which render node to use (e.g. `/dev/dri/renderD129`). On some systems this is required.

**NVENC:** Fast encoding, but quality may be lower on older GPUs. Nvidia drivers must be installed.

#### Rylus as Second Screen

There are several ways to use Rylus to turn your tablet into a second screen.

##### Intel GPU on Xorg with Intel Drivers

Intel's drivers support creating virtual outputs configurable via xrandr.

**Warning:** The following configuration can break X server startup. Make sure you know how to recover from a broken X configuration before proceeding.

Install the `xf86-video-intel` driver and create `/etc/X11/xorg.conf.d/20-intel.conf`:
```text
Section "Device"
    Identifier "intelgpu0"
    Driver "intel"

    # adds two virtual monitors
    Option "VirtualHeads" "2"

    # if your screen flickers, try uncommenting one of:
    # Option "TripleBuffer" "true"
    # Option "TearFree"     "true"
    # Option "DRI"          "false"
EndSection
```

After a reboot, `xrandr` will show `VIRTUAL1` and `VIRTUAL2`. To activate a virtual monitor at 1112x834 @ 60 Hz:
```console
> gtf 1112 834 60

  # 1112x834 @ 60.00 Hz (GTF) hsync: 51.78 kHz; pclk: 75.81 MHz
  Modeline "1112x834_60.00"  75.81  1112 1168 1288 1464  834 835 838 863  -HSync +Vsync
> xrandr --newmode "1112x834_60.00"  75.81  1112 1168 1288 1464  834 835 838 863  -HSync +Vsync
> xrandr --addmode VIRTUAL1 1112x834_60.00
> xrandr --output VIRTUAL1 --mode 1112x834_60.00
```

Configure this monitor in your system settings like a regular second display. In Rylus, select it from the capture menu. You may want to enable cursor display.

##### Dummy Plugs

HDMI, DisplayPort, or VGA dummy plugs are inexpensive devices that simulate a connected monitor. Plug one in, configure the additional display in your system settings, then select it in Rylus.

##### Other Options

The following are less tested — contributions to expand this documentation are welcome:
- On Wayland with Sway, `create_output` can [create headless outputs](https://github.com/swaywm/sway/releases/tag/1.5) (see [sway#5553](https://github.com/swaywm/sway/issues/5553))
- On Wayland with GNOME, mutter supports [virtual monitors](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/1698)

#### Encryption

Rylus enables TLS by default (`--tls-mode auto`): a self-signed certificate is generated on first run and reused on subsequent starts. Your browser will warn that the certificate is untrusted — this is expected for a self-signed certificate and can be accepted.

To use a CA-signed certificate instead (no browser warning):

```sh
rylus --tls-mode certified --tls-cert-path /path/to/cert.pem --tls-key-path /path/to/key.pem
```

To run without TLS (e.g. behind a reverse proxy that handles it):

```sh
rylus --tls-mode disabled
```

Note for Firefox users: a [known bug](https://bugzilla.mozilla.org/show_bug.cgi?id=1187666) prevents accepting self-signed certificates for WebSocket connections when the TLS warning is shown. As a workaround, visit the Rylus URL directly in the address bar first and accept the certificate, then reload the page normally.

### macOS

Download `Rylus-<version>-universal.dmg` from the [releases page](https://github.com/Chorosyne/rylus/releases), open it, and drag `Rylus.app` into `/Applications`. The DMG is notarized, so Gatekeeper should let it open without extra steps.

On first launch, macOS will prompt for the permissions below — grant them via System Settings → Privacy & Security if you decline the prompt initially:
- Incoming connections
- Screen capturing
- Controlling your desktop

#### Hardware Acceleration

Rylus can use the VideoToolbox framework on macOS for hardware accelerated encoding. Video quality may be lower than software encoding, so VideoToolbox is disabled by default.

### Windows

#### Hardware Acceleration

Rylus supports Nvidia's NVENC and Microsoft's MediaFoundation for hardware accelerated encoding. Due to varying quality across hardware, both are disabled by default.

## Building

Building Rylus requires Rust, make, git, a C compiler (for FFmpeg and system library linkage), nasm, and Node.js (esbuild handles TypeScript compilation automatically via build.rs).

```sh
cargo build            # debug build
cargo build --release  # release build (LTO enabled)
```

### Linux Dependencies

**Debian/Ubuntu:**
```sh
sudo apt-get install -y \
  libpipewire-0.3-dev libdbus-1-dev \
  libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev \
  libswscale-dev libswresample-dev \
  libwayland-dev libxkbcommon-dev libdrm-dev libva-dev \
  pkg-config clang nasm
```

**Fedora:**
```sh
sudo dnf install pipewire-devel dbus-devel \
  ffmpeg-devel libdrm-devel libva-devel \
  wayland-devel libxkbcommon-devel \
  pkg-config clang nasm npm
```

**Arch Linux:**
```sh
sudo pacman -S pipewire ffmpeg libva libdrm \
  wayland libxkbcommon pkg-config clang nasm
```

On Windows, only MSVC is supported as the C compiler.

### Docker

A Dockerfile for building the Linux version is located at [docker/Dockerfile](docker/Dockerfile):

```sh
docker build -t rylus-build docker/
docker run -it rylus-build bash
root@container:/# git clone https://github.com/Chorosyne/rylus
root@container:/# cd rylus/
root@container:/rylus# cargo build --release
```

Copy the binary out of the container:
```sh
docker cp <container-id>:/rylus/target/release/rylus ~/rylus
```

To build a .deb package, install `cargo-deb` inside the container and run `cargo deb`.

## How Does This Work?

### Stylus and Touch

Modern browsers expose [PointerEvents](https://developer.mozilla.org/en-US/docs/Web/API/PointerEvent) that carry not only mouse position but also stylus pressure, tilt, and touch contact geometry. Rylus serves a web client that captures these events and sends them to the server over a WebSocket connection (with auto-reconnect and exponential backoff).

On the server side, Rylus processes these events through either the generic OS-independent backend (mouse control only) or, on Linux, the uinput backend. The uinput backend uses the kernel's uinput module to create virtual input devices — mouse, stylus, and multi-touch — that applications see as real hardware.

### Screen Mirroring and Window Capture

On Linux with X11, Rylus connects to the X server using [x11rb](https://github.com/psychon/x11rb) for window enumeration and screen capture. The MIT-SHM extension provides shared memory image transfer for fast capture without copying pixel data over the socket.

On Linux with Wayland, [pipewire-rs](https://gitlab.freedesktop.org/pipewire/pipewire-rs) captures the screen through the xdg-desktop-portal. Portal sessions are cached to avoid redundant D-Bus roundtrips and compositor permission prompts. DMA-BUF and MemFd buffers are explicitly synchronized before access.

On macOS, CoreGraphics handles screen capture. On Windows, GDI capture is used via Microsoft's official [windows](https://github.com/microsoft/windows-rs) crate.

Captured frames are encoded to an H.264 video stream using FFmpeg (linked via ffmpeg-sys-next). The stream is packaged in fragmented MP4 containers so browsers can play it through the [Media Source Extensions](https://developer.mozilla.org/en-US/docs/Web/API/Media_Source_Extensions_API) API. H.264 is chosen for broad hardware and browser support.

## FAQ

**Q: The page does not load on my tablet / I get a timeout.**
A: A firewall is likely blocking the connection. Open port 1701 (or whichever port you configured with `--web-port`). The WebSocket stream is served on the same port.

**Q: I get `ERROR Failed to create uinput device: CError: code...`**
A: uinput is probably misconfigured. Verify you followed all [setup steps](#linux) and logged out and back in. Very old kernels may not support the required features — upgrade your system if needed.

**Q: The "Capture" dropdown is empty and the screen is not mirrored.**
A: Check that port 1701 (or your `--web-port`) is open in your firewall. The WebSocket stream is served on the same port as the HTTP interface.

**Q: I can only see the whole screen in the "Capture" dropdown, not individual windows.**
A: On macOS and Windows, per-window capture is not implemented. On Linux, your window manager may not support [Extended Window Manager Hints](https://specifications.freedesktop.org/wm-spec/latest/) or you may need to enable them (e.g. in XMonad).

**Q: Do I have to set up Rylus as a second screen?**
A: No, the second screen setup is entirely optional. Rylus works as a mirror/input device without it.

**Q: I cannot connect my tablet to the URL Rylus displays.**
A: Your computer and tablet may be on different networks. Verify they are on the same local network.

**Q: This does not work in Firefox for Android.**
A: It does — make sure Firefox 80 or later is installed.

**Q: This does not work in Chrome on my iPad.**
A: Chrome on iPadOS/iOS lacks some video streaming features. Use Firefox or Safari instead.

**Q: Can I use Rylus without WiFi?**
A: Yes. Options include:
- Create a WiFi hotspot on your tablet and connect your computer to it
- Use USB tethering for a direct peer-to-peer connection
- On Android, use ADB port forwarding:
  ```console
  adb reverse tcp:1701 tcp:1701
  ```
  Then connect from your Android device to `http://127.0.0.1:1701` (or `https://` if TLS is enabled).

Rylus only requires IP connectivity between the two devices — WiFi is one option among several.

---

Rylus is a fork of [Weylus](https://github.com/H-M-H/Weylus) by H-M-H, licensed under [AGPL-3.0-or-later](LICENSE).
