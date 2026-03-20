# Rylus

![Build](https://github.com/revelri/Rylus/workflows/Build/badge.svg)

Rylus turns your tablet or smartphone into a graphic tablet/touch screen for your computer.

> Rylus is a fork of [H-M-H/Weylus](https://github.com/H-M-H/Weylus), rewritten with a pure Rust
> architecture. All C/C++ code has been replaced with native Rust crates, GStreamer has been replaced
> with direct PipeWire bindings, FLTK with egui, and the monolith has been refactored into a
> 7-crate Cargo workspace.

## Table of Contents
* [Features](#features)
* [Architecture](#architecture)
* [Installation](#installation)
    * [Packages](#packages)
* [Running](#running)
    * [Fullscreen](#fullscreen)
    * [Keyboard Input](#keyboard-input)
    * [Automation](#automation)
    * [Linux](#linux)
        * [Wayland](#wayland)
        * [Hardware Acceleration](#hardware-acceleration)
        * [Rylus as Second Screen](#rylus-as-second-screen)
            * [Intel GPU on Xorg with Intel drivers](#intel-gpu-on-xorg-with-intel-drivers)
            * [Dummy Plugs](#dummy-plugs)
            * [Other Options](#other-options)
        * [Encryption](#encryption)
    * [macOS](#macos)
        * [Hardware Acceleration](#hardware-acceleration-1)
    * [Windows](#windows)
        * [Hardware Acceleration](#hardware-acceleration-2)
* [Building](#building)
    * [Docker](#docker)
* [How does this work?](#how-does-this-work)
    * [Stylus/Touch](#stylustouch)
    * [Screen mirroring & window capturing](#screen-mirroring--window-capturing)
* [FAQ](#faq)

## Features
- Control your mouse with your tablet
- Mirror your screen to your tablet
- Send keyboard input using physical keyboards
- Hardware accelerated video encoding
- Pure Rust codebase — no custom C/C++ code
- Native desktop GUI built with [egui](https://github.com/emilk/egui)
- WebSocket auto-reconnect with exponential backoff
- Access code authentication with argon2 hashing and rate limiting

The above features are available on all operating systems but Rylus works best on Linux. Additional
features on Linux are:
- Support for a stylus/pen (supports pressure and tilt)
- Multi-touch: Try it with software that supports multi-touch, like Krita, and see for yourself!
- Capturing specific windows and only drawing to them
- Faster screen mirroring
- Tablet as second screen

## Architecture

Rylus is organized as a Cargo workspace with seven crates:

| Crate | Purpose |
|-------|---------|
| **rylus-core** | Config, protocol definitions, error types, pixel formats, traits |
| **rylus-capture** | Screen capture backends (X11 via x11rb, PipeWire for Wayland, CoreGraphics, Windows GDI) |
| **rylus-encode** | Video encoding via FFmpeg (H.264, fMP4 container) |
| **rylus-input** | Input device backends (Linux uinput/evdev, Windows WinRT, macOS enigo) |
| **rylus-transport** | WebSocket transport layer |
| **rylus-gui** | Desktop GUI (egui/eframe) |
| **rylus-server** | HTTP/WebSocket server, session management, main binary |

## Installation
Grab the latest release for your OS from the
[releases page](https://github.com/revelri/Rylus/releases) and install it on your computer. No apps
except a modern browser (Firefox 80+, Safari on iOS/iPadOS 13+) are required on your tablet. **If
you run Linux make sure to follow the instructions described [here](#linux) to enable uinput for
features like pressure sensitivity and multitouch!**

### Packages
AUR packages are available:
- From source: [weylus](https://aur.archlinux.org/packages/weylus/)
- Prebuilt binary: [weylus-bin](https://aur.archlinux.org/packages/weylus-bin/)

## Running
Start Rylus, optionally set an access code in the settings panel, and press Start. This starts a
webserver on your computer. To control your computer with your tablet, open
`http://<address of your computer>:<port, default 1701>` in a browser on your tablet. Rylus displays
the URL and a QR code you can scan. If you have a firewall, open TCP ports for the webserver (1701
by default) and the websocket connection (9001 by default).

On many Linux distributions this is done with ufw:
```
sudo ufw allow 1701/tcp
sudo ufw allow 9001/tcp
```

Rylus supports access code authentication (hashed with argon2, rate-limited) but does not use TLS
by default. Only run on networks you trust, or set up a TLS proxy (see [Encryption](#encryption)).

### Fullscreen
You may want to add a bookmark to your home screen on your tablet as this enables running Rylus in
full screen mode (on iOS/iPadOS this needs to be done with Safari). If you are not on iOS/iPadOS
there is a button to toggle full screen mode.

### Keyboard Input
Rylus supports keyboard input for physical keyboards, so if you have a Bluetooth keyboard, just
connect it to your tablet and start typing. Due to technical limitations onscreen keyboards are not
supported.

### Automation
Rylus provides a command-line interface; `--no-gui` starts Rylus in headless mode. For more options
see `rylus --help`. Configuration is stored in `~/.config/rylus/rylus.toml`.

You can enable more verbose logging by setting the environment variable `RYLUS_LOG_LEVEL` to
`DEBUG` or `TRACE` as well as `RYLUS_LOG_JSON` to `true` for JSON logging.

### Linux
Rylus uses the `uinput` interface to simulate input events on Linux. **To enable stylus and
multi-touch support `/dev/uinput` needs to be writable by Rylus.** To make `/dev/uinput`
permanently writable by your user, run:
```sh
sudo groupadd -r uinput
sudo usermod -aG uinput $USER
echo 'KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"' \
| sudo tee /etc/udev/rules.d/60-rylus.rules
```

Then, either reboot, or run

```sh
sudo udevadm control --reload
sudo udevadm trigger
```

then log out and log in again. To undo this, run:

```sh
sudo rm /etc/udev/rules.d/60-rylus.rules
```

This allows your user to synthesize input events system-wide, even when another user is logged in.
Therefore, untrusted users should not be added to the uinput group.

#### Wayland
Rylus supports Wayland via direct PipeWire bindings and the xdg-desktop-portal. Install `pipewire`
and `xdg-desktop-portal` as well as one of:
- `xdg-desktop-portal-gtk` for GNOME
- `xdg-desktop-portal-kde` for KDE
- `xdg-desktop-portal-wlr` for wlroots-based compositors like Sway

There are still some things that do not work:
- input mapping for windows
- displaying proper window names
- capturing the cursor

#### Hardware Acceleration
On Linux Rylus supports hardware accelerated video encoding through the Video Acceleration API
(VAAPI) or Nvidia's NVENC. By default hardware acceleration is disabled as quality and stability of
the hardware encoded video stream varies widely among different hardware and sufficient quality can
not be guaranteed. If VAAPI is used it is possible to select a specific driver by setting the
environment variable `LIBVA_DRIVER_NAME`. You can find possible values with the command
`ls /usr/lib/dri/ | sed -n 's/^\(\S*\)_drv_video.so$/\1/p'`. On some distributions the drivers may
not reside in `/usr/lib/dri` but for example in `/usr/lib/x86_64-linux-gnu/dri` and may not be found
by Rylus. To force Rylus to search another directory for drivers, the environment variable
`LIBVA_DRIVERS_PATH` can be set.
Additionally you can specify the VAAPI device to use by setting `RYLUS_VAAPI_DEVICE`; by default
devices can be found in `/dev/dri`. On some systems this is not optional and this variable must be
set. If VAAPI doesn't work out of the box for you, have a look into `/dev/dri`, often setting
`RYLUS_VAAPI_DEVICE=/dev/dri/renderD129` is already the solution. Note that you may need to install
the driver(s) first.

Nvidia's NVENC is very fast but may deliver a video stream of lower quality on older GPUs. More
recent GPUs should provide higher quality. Nvidia drivers need to be installed.

#### Rylus as Second Screen
There are a few possibilities to use Rylus to turn your tablet into a second screen.

##### Intel GPU on Xorg with Intel drivers
Intel's drivers support creating virtual outputs that can be configured via xrandr.

But first a word of warning: The following configuration may break starting the X server. This means
you might end up without a graphical login or X may get stuck and just display a black screen. So
make sure you know what you are doing or are at least able to recover from a broken X server.

You will need to install the `xf86-video-intel` driver and create the file
`/etc/X11/xorg.conf.d/20-intel.conf` with the following contents:
```text
Section "Device"
    Identifier "intelgpu0"
    Driver "intel"

    # this adds two virtual monitors / devices
    Option "VirtualHeads" "2"

    # if your screen is flickering one of the following options might help
    # Option "TripleBuffer" "true"
    # Option "TearFree"     "true"
    # Option "DRI"          "false"
EndSection
```
After a reboot `xrandr` will show two additional monitors `VIRTUAL1` and `VIRTUAL2` and can be used
to configure them. To activate `VIRTUAL1` with a screen size of 1112x834 and a refresh rate of 60
fps the following commands can be used:
```console
> # this generates all input parameters xrandr needs
> #from a given screen resolution and refresh rate
> gtf 1112 834 60

  # 1112x834 @ 60.00 Hz (GTF) hsync: 51.78 kHz; pclk: 75.81 MHz
  Modeline "1112x834_60.00"  75.81  1112 1168 1288 1464  834 835 838 863  -HSync +Vsync
> # setup the monitor
> xrandr --newmode "1112x834_60.00"  75.81  1112 1168 1288 1464  834 835 838 863  -HSync +Vsync
> xrandr --addmode VIRTUAL1 1112x834_60.00
> xrandr --output VIRTUAL1 --mode 1112x834_60.00
> # check if everything is in order
> xrandr
```
Now you should be able to configure this monitor in your system setting like a regular second
monitor and for example set its position relative to your primary monitor.

After setting up the virtual monitor start Rylus and select it in the capture menu. You may want to
enable displaying the cursor in this case. That is it!

##### Dummy Plugs
Rylus detects if you use multiple monitors and you can select the one you want to mirror. So if you
want to use Rylus as a second screen you could just buy another monitor. Obviously this is
pointless as if you already bought that monitor, there is no need to use Rylus! This is where so
called **HDMI/Displayport/VGA Dummy Plugs** come in handy. These are small devices that pretend to
be a monitor but only cost a fraction of the price of an actual monitor.

Once you have bought one and plugged it into your computer you can configure an additional screen
just like you would do with an actual one and then use Rylus to mirror this virtual screen.

##### Other Options
The following is untested/incomplete, feel free to do more research and open a pull request to
expand documentation on this!
- On Wayland with sway there is `create_output` which can be used to [create headless
  outputs](https://github.com/swaywm/sway/releases/tag/1.5), unfortunately it is not documented how
  to actually do that: https://github.com/swaywm/sway/issues/5553
- On Wayland with GNOME recently there has been added an option to [create virtual monitors with
  mutter](https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/1698)

#### Encryption
By default Rylus comes without encryption and should only be run on networks you trust. If this is
not the case it's strongly advised to set up a TLS proxy. One option is to use
[hitch](https://hitch-tls.org/), an example script that sets up encryption is located at
`rylus_tls.sh`.
But any TLS proxy should work just fine.

Note that the mentioned script works by creating a self-signed certificate. This means your browser
will most likely display a scary looking but completely unfounded message telling you how incredibly
dangerous it is to trust the certificate you yourself just created; this can be safely ignored!

In case you are using Firefox: There is a [bug](https://bugzilla.mozilla.org/show_bug.cgi?id=1187666)
that prevents users from accepting self-signed certificates for websocket connections. A workaround
is to directly open the websocket connection via the URL bar and accept the certificate there. After
accepting the connection will of course fail as the browser expects https and not wss as protocol.

### macOS
Rylus needs some permissions to work properly, make sure you enable:
- Incoming connections
- Screen capturing
- Controlling your desktop

#### Hardware Acceleration
Rylus can make use of the VideoToolbox framework on macOS for hardware acceleration. Video quality
may be worse than software encoding, so VideoToolbox is disabled by default.

### Windows

#### Hardware Acceleration
Rylus can make use of Nvidia's NVENC as well as Microsoft's MediaFoundation for hardware accelerated
video encoding. Due to widely varying quality it is disabled by default.

## Building
To build Rylus you need Rust, TypeScript, make, git, a C compiler, nasm, and bash. `cargo build`
builds the project. For a release build run `cargo build --release`.

On Linux the following dependencies are required:

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

After npm is installed, TypeScript must be installed:
```sh
sudo npm install typescript -g
```

On Windows only msvc is supported as C compiler.

### Docker
It is also possible to build the Linux version inside a docker container. The Dockerfile used is
located at [docker/Dockerfile](docker/Dockerfile). Building works like this:
```console
docker run -it hhmhh/weylus_build bash
root@container:/# git clone https://github.com/revelri/Rylus
root@container:/# cd Rylus/
root@container:/Rylus# cargo deb
```
Once the build is finished you can copy the binary from the container to your file system:
```sh
docker cp <container-id>:/Rylus/target/release/rylus ~/some/path/rylus
```
The .deb is located at `/Rylus/target/debian/`.

## How does this work?
### Stylus/Touch
Modern browsers expose
[PointerEvents](https://developer.mozilla.org/en-US/docs/Web/API/PointerEvent) that convey not
only mouse but additionally stylus/pen and touch information. Rylus sets up a webserver with
TypeScript code to capture these events. The events are sent back to the server via WebSockets
(with auto-reconnect and exponential backoff).

Rylus then processes these events using either the generic OS-independent backend (mouse control
only) or on Linux the uinput backend, which uses the uinput kernel module to create a wide range
of input devices including mouse, stylus, and touch input devices.

### Screen mirroring & window capturing
On Linux, Rylus uses [x11rb](https://github.com/psychon/x11rb) to connect to the X server for
window information and screen capture. The MIT-SHM extension provides shared memory images via
`XShmCreateImage` for fast capture. On Wayland, [pipewire-rs](https://gitlab.freedesktop.org/pipewire/pipewire-rs)
captures the screen directly through the xdg-desktop-portal, without the GStreamer dependency chain
that the original Weylus required.

On macOS, CoreGraphics handles screen capture. On Windows, GDI capture is used via Microsoft's
official [windows](https://github.com/microsoft/windows-rs) crate.

Captured images are encoded to an H.264 video stream using FFmpeg (via ffmpeg-sys-next). Fragmented
MP4 is used as the container format to enable browsers to play the stream via the Media Source
Extensions API. H.264 is used for its wide support and fast encoding. FFmpeg is statically linked
into the binary.

## FAQ
Q: Why does the page not load on my tablet and instead I get a timeout?<br>
A: There probably is some kind of firewall running, make sure the ports Rylus uses are opened.

Q: Why do I get the error `ERROR Failed to create uinput device: CError: code...`?<br>
A: uinput is probably misconfigured, have you made sure to follow all instructions and logged out
and in again? You may also be running a very old kernel that does not support the required features.
In that case try to upgrade your system or use a newer one.

Q: Why is the "Capture" drop down empty and the screen not mirrored?<br>
A: It is possible that only the port for the webserver but not the websocket has been opened, check
that both ports have been opened.

Q: Why can I not select any windows in the "Capture" drop down and only see the whole screen?<br>
A: If you are running Rylus on macOS or Windows this feature is unfortunately not implemented. On
Linux it is possible that your window manager does not support
[Extended Window Manager Hints](https://specifications.freedesktop.org/wm-spec/latest/) or that you
need to activate them first, like for XMonad.

Q: Do I have to follow the instructions to setup Rylus as second screen too?<br>
A: No, this is strictly optional.

Q: Why am I unable to connect my tablet to the URL displayed by Rylus?<br>
A: It is possible that your computer and WiFi connected tablet are on different networks, make sure
they are on the same network.

Q: Why does this not run on Firefox for Android?<br>
A: Actually it does, just make sure Firefox version 80+ is installed.

Q: Why does this not run under Chrome on my iPad?<br>
A: Chrome lacks some features for video streaming on iPadOS/iOS, try Firefox or Safari.

Q: Can I use Rylus even if there is no WiFi?<br>
A: Probably yes! Most tablets permit setting up a WiFi hotspot that can be used to connect your
computer and tablet. Alternatively there is USB tethering too, which can be used to setup a peer to
peer connection between your tablet and computer over USB. Another method for Android devices is to
setup a socket connection with
[adb](https://developer.android.com/studio/command-line/adb#Enabling):
```console
adb reverse tcp:1701 tcp:1701
adb reverse tcp:9001 tcp:9001
```
Like that you can connect from your Android device to Rylus with the URL: `http://127.0.0.1:1701`.

Rylus only requires that your devices are connected via the Internet Protocol and that doesn't
necessarily imply WiFi.

---

[![Packaging status](
https://repology.org/badge/vertical-allrepos/weylus.svg
)](https://repology.org/project/weylus/versions)
