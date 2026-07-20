# Rylus wire protocol

This document specifies the wire protocol between a Rylus desktop server and a
browser client, as implemented by `MessageInbound`/`MessageOutbound` in
[`crates/rylus-core/src/protocol.rs`](../crates/rylus-core/src/protocol.rs).
That module is the source of truth; if this document and the code disagree,
the code wins and this document is out of date.

The protocol exists to carry two things over a single connection: a live video
mirror of the desktop (server → client) and pointer/keyboard input plus
control messages (client ↔ server). It is designed around AX-1 (perceived
stylus latency is the product) and AX-3 (a single LAN-only WebSocket is the
v1 transport contract) — see
[`VISION.md`](../VISION.md) and
[ADR-0003](adr/ADR-0003-lan-only-websocket-transport-v1.md). This document is
one of the deliverables VISION.md names for 1.0.0: "message types, framing,
heartbeat v3, keyframe-on-demand, and RTT sampling".

This document does not cover implementing an alternative *server*: it
describes the wire contract a client (or a reimplementation of either side)
must honor.

## 1. Connection lifecycle

### 1.1 Before the WebSocket upgrade

The browser first loads the server's HTTP page at `/`. If the server is
configured with an access code, the server serves an access-code form and the
client must POST a correct code (verified with a constant-time argon2 check,
`crates/rylus-server/src/web.rs`) before it is issued a session; incorrect
attempts are rate-limited per IP. This HTTP layer is out of scope for the
message protocol below but is part of the real connection lifecycle a client
must drive.

The client then opens a WebSocket to `/ws`:

```js
const protocol = location.protocol === "https:" ? "wss://" : "ws://";
const ws = new WebSocket(protocol + location.hostname + ":" + location.port + "/ws");
ws.binaryType = "arraybuffer";
```

The server validates the upgrade request's `Origin` header against `Host`
(`crates/rylus-server/src/web.rs`): if `Origin` is present and its authority
doesn't match `Host`, the upgrade is rejected. Non-browser clients that omit
`Origin` entirely are accepted at this layer.

### 1.2 Hello handshake

Once the socket is open, the client sends `Hello` and the server echoes
`Hello` back with its own version:

```json
{"Hello": {"protocol_version": 3}}
```

Two constants in `protocol.rs` govern this exchange:

```rust
/// Current protocol version.
///
/// Increment when adding new message types or changing semantics.
/// Clients and server negotiate on the minimum of their versions.
pub const PROTOCOL_VERSION: u32 = 3;

/// Minimum client protocol version the server will accept.
///
/// A client below this version receives a `HelloNack` and must upgrade.
/// Bump this when a wire-breaking change ships and old clients would
/// silently misinterpret new messages.
pub const MIN_CLIENT_PROTOCOL_VERSION: u32 = 2;
```

Server-side handling (`RylusClientHandler::handle_hello`,
`crates/rylus-server/src/session.rs`):

- If `hello.protocol_version < MIN_CLIENT_PROTOCOL_VERSION`, the server sends
  `HelloNack` (see below) and stops — it does **not** also send `Hello`.
- Otherwise the server computes `negotiated = PROTOCOL_VERSION.min(hello.protocol_version)`
  (logged, not currently sent to the client) and replies with its own `Hello`
  carrying `PROTOCOL_VERSION`.

So the negotiation rule is: the server accepts any client whose declared
version is `>= MIN_CLIENT_PROTOCOL_VERSION`, and the two sides implicitly
operate at `min(client_version, server_version)`. There is no per-message
version tagging; a version bump that changes wire semantics for existing
message types requires bumping `MIN_CLIENT_PROTOCOL_VERSION` so old clients
are refused rather than silently misinterpreting new behavior.

The client re-sends `Hello` (along with `GetCapturableList`, video pause
state, and its `Config`) every time a WebSocket opens — including after a
reconnect — since each new socket is a fresh handshake
(`ts/lib.ts`, `webSocket.onopen`).

### 1.3 `HelloNack` — version-guard path

```json
{
  "HelloNack": {
    "server_version": 3,
    "min_client_version": 2,
    "reason": "Client is too old — reload the page to pick up the new bundle."
  }
}
```

Sent instead of `Hello` when the client's declared `protocol_version` is
below `MIN_CLIENT_PROTOCOL_VERSION`. It is a distinct, typed message (not a
generic `Error`) specifically so the client can special-case the reload
prompt rather than just showing a toast. The reference client
(`ts/lib.ts`) shows a toast with `reason` and reloads the page after a short
delay:

```js
} else if ("HelloNack" in msg) {
    const nack = msg["HelloNack"];
    showToast(nack.reason || "This page is out of date — reloading to pick up the new client.", "error", 4500);
    setTimeout(() => location.reload(), 2000);
}
```

A page reload is the expected recovery because the client bundle itself is
versioned alongside the server and a reload fetches the current bundle from
the same server.

### 1.4 Post-handshake bring-up

After the `Hello`/`Hello` (or `Hello`/`HelloNack`) exchange, the reference
client immediately sends, in order: `GetCapturableList`, optionally
`PauseVideo` (if video is disabled in client settings), its `Config`, and
`RequestKeyframe`. It also starts its heartbeat timer at this point (see
§3.1). The same sequence runs again after every reconnect.

### 1.5 Idle timeout and disconnect

The server closes a WebSocket after `IDLE_TIMEOUT` (120 seconds,
`crates/rylus-transport/src/websocket.rs`) of no inbound frames. Any inbound
message resets this timer, including `Heartbeat`, so a client sending
heartbeats every 5 seconds (§3.1) stays comfortably inside the idle budget.
On disconnect (error or close), the reference client tears down local input
handlers and drives a reconnect state machine with exponential backoff
(base 1 s, max 30 s, capped at 10 attempts, `ts/lib.ts`); reconnection is
part of the protocol's reliability story per AX-7, even though the backoff
schedule itself is a client policy, not a wire message.

## 2. Framing

The connection carries exactly two frame kinds, and they are never mixed:

- **Text frames** carry control messages: a single JSON value, either a bare
  string (for unit-variant messages, e.g. `"Heartbeat"`) or a JSON object
  keyed by variant name (for messages carrying data, e.g.
  `{"ClientRtt": {"rtt_ms": 42}}`). These deserialize as `MessageInbound`
  (client → server) or `MessageOutbound` (server → client) via serde's
  default externally-tagged enum representation — see §2.1. The server
  enforces `MAX_TEXT_FRAME_SIZE` (64 KiB,
  `crates/rylus-transport/src/websocket.rs`) and silently drops any larger
  text frame with a warning log rather than attempting to parse it.
- **Binary frames** carry raw fragmented-MP4 (fMP4) video segments, sent
  server → client only. They are never JSON and never contain control data.
  A binary frame's payload is fed directly into the browser's MSE
  `SourceBuffer` (or, on the WebCodecs decode path, into a
  `WebCodecs`/`EncodedVideoChunk` pipeline) — see
  `crates/rylus-transport/src/websocket.rs` (`Message::Binary(data.into())`)
  and `ts/lib.ts` (`handle_messages`, the `typeof msg == "object"` branch is
  reached only for parsed JSON; anything else falls through to
  `webcodecs_pipe.feed(event.data)` / `queue.push(event.data)`).

So a receiver distinguishes control from video purely by WebSocket frame
type (`Message::Text` vs `Message::Binary`), not by any in-band tag. The
server's outbound writer task prioritizes draining queued video frames ahead
of any pending control message on every loop iteration
(`crates/rylus-transport/src/websocket.rs`, `rylus_websocket_channel`) because
video is the latency-sensitive payload; video is queued in a bounded
drop-oldest ring (`VIDEO_QUEUE_CAPACITY = 4`) rather than blocking the
encode thread, and `WsRylusSender::dropped_video_frames()` tracks how many
frames that queue has discarded.

`Message::Ping`/`Message::Pong`/`Message::Frame` are accepted at the
WebSocket-protocol level but ignored by the Rylus message layer — they carry
no Rylus semantics; keepalive is done at the application layer by `Heartbeat`
(§3.1), not WebSocket ping/pong.

### 2.1 JSON shape (serde tagging)

`MessageInbound` and `MessageOutbound` are plain Rust enums with serde's
default (externally tagged) representation:

- A unit variant (no fields) serializes as a bare JSON string equal to the
  variant name, e.g. `MessageInbound::Heartbeat` → `"Heartbeat"`,
  `MessageInbound::GetCapturableList` → `"GetCapturableList"`.
- A variant with a single tuple field serializes as `{"VariantName": <value>}`,
  e.g. `MessageInbound::Hello(Hello { protocol_version: 3 })` →
  `{"Hello": {"protocol_version": 3}}`.
- A struct-like variant (named fields inline on the variant) serializes the
  same way, with the fields as an object, e.g.
  `MessageInbound::ClientRtt { rtt_ms: 42 }` → `{"ClientRtt": {"rtt_ms": 42}}`.
- `MessageInbound::BatchedPointerEvents(Vec<PointerEvent>)` carries an
  explicit `#[serde(rename = "batched_pointer_events")]`, so its wire key is
  lowercase `batched_pointer_events` rather than the Rust variant name — the
  one exception to "wire key == Rust variant name" in this protocol.

An unrecognized variant name, or a variant object missing required fields,
fails to deserialize and the frame is dropped with a warning
(`crates/rylus-transport/src/websocket.rs`); there is no error frame sent
back to the peer for a malformed *inbound* control message (as opposed to
`MessageOutbound::Error`, which is an explicit application-level error the
server sends deliberately — see §4.2.6).

## 3. `MessageInbound` — client → server

All variants below are declared on `MessageInbound` in `protocol.rs`.

### 3.1 `Heartbeat` / `HeartbeatAck` (round-trip keepalive)

```json
"Heartbeat"
```

The client sends a bare `Heartbeat` on a fixed interval. The `protocol.rs`
declaration carries no interval; the interval is a client policy defined in
the reference client (`ts/lib.ts`):

```js
const HEARTBEAT_INTERVAL = 5000  // 5s — well within the idle timeout, fast enough to
                                  // keep the connection-quality pip responsive.
```

The client sends the first heartbeat immediately on connect/reconnect (so the
connection-quality indicator isn't blank for the first interval), then every
5 seconds thereafter, and stops the timer on disconnect.

On receipt, the server (`RylusClientHandler::run`, `session.rs`) replies with:

```json
{"HeartbeatAck": {"server_ts_ms": 1737313200123}}
```

```rust
/// Echo of a client Heartbeat. `server_ts_ms` is the server's receive
/// timestamp in milliseconds since UNIX epoch. The client subtracts its
/// own send time to derive RTT for the connection-quality indicator.
HeartbeatAck {
    server_ts_ms: u64,
},
```

Receiving *any* inbound frame (not just `Heartbeat`) resets the server's idle
timer (§1.5); `Heartbeat` exists specifically so the connection is kept alive
and measured even when the user is idle and no input/video events are
otherwise flowing.

The client does not use `server_ts_ms` directly for RTT — it measures RTT
locally as wall-clock time between sending `Heartbeat` and receiving the
matching `HeartbeatAck` (`performance.now()` delta,
`ts/lib.ts:on_heartbeat_ack`), keeps the last 8 samples, and derives both a
smoothed RTT and a jitter figure (variance of those samples) that feed a
three-tier connection-quality classification (`good` / `fair` / `poor`,
thresholds in `ts/utils.ts::calculateConnectionQuality`: good is
`rtt < 50ms && jitter < 5ms`, fair is `rtt < 150ms && jitter < 20ms`,
otherwise poor) shown as a colored status pip in the UI. `server_ts_ms` is
available for future clock-skew-aware measurement but is not currently
consumed by the reference client beyond being present on the wire.

### 3.2 `ClientRtt { rtt_ms }`

```json
{"ClientRtt": {"rtt_ms": 42}}
```

```rust
/// Client-measured round-trip time (milliseconds) derived from the
/// Heartbeat/HeartbeatAck pair. The server uses this to shape rate
/// control decisions: high RTT tightens the latency budget.
ClientRtt {
    rtt_ms: u32,
},
```

Sent immediately after the client computes RTT from a `HeartbeatAck` (i.e.
one `ClientRtt` per heartbeat round-trip, roughly every 5 seconds). The
server forwards it to the video/encode thread as `VideoCommands::ClientRtt`
(`crates/rylus-server/src/session.rs`), where it feeds rate-control decisions
— a high measured RTT tightens the encoder's latency budget. This is the
mechanism by which client-observed network quality closes the loop back into
server-side encode behavior, per ADR-0003's "RTT-aware rate control" latency
strategy.

### 3.3 `RequestKeyframe`

```json
"RequestKeyframe"
```

```rust
/// Client asks the server to emit an IDR on the next encoded frame.
/// Used on reconnect, tab refocus, and decode-error recovery so the
/// client doesn't have to wait for the next natural GOP boundary.
RequestKeyframe,
```

The server forwards this to the video/encode thread as
`VideoCommands::RequestKeyframe` (`session.rs`), which causes the encoder to
emit an IDR (keyframe) on its next output frame instead of a delta frame.
The reference client sends `RequestKeyframe` in exactly the cases the doc
comment names:

- Immediately after (re)connecting, right after sending `Hello` and `Config`
  (both the initial connect path and the reconnect path in `ts/lib.ts`), so a
  fresh socket doesn't have to wait out a full GOP before the first frame
  paints.
- On `document.onvisibilitychange` when the tab becomes visible again (and
  video is enabled), paired with `ResumeVideo` — avoids drawing a frozen
  frame from a stale P-frame chain built while the tab was hidden.
- On a WebCodecs decode error (the pipe's error callback tears down and
  rebuilds the WebCodecs surface and asks for a fresh IDR to reseed it), and
  once more right after WebCodecs decode is first set up for a given
  `VideoInit` (§4.2.3), since the decoder has no prior frames to reference.

### 3.4 `HelloNack` version-guard (client-observed behavior)

Covered fully in §1.3; from the client's perspective this is not something it
sends, but the terminal state of an outbound `Hello` when its declared
version is rejected. Documented here for completeness since it's one of the
version-guard behaviors this protocol defines around the `Hello` exchange.

### 3.5 `Hello(Hello)`

```json
{"Hello": {"protocol_version": 3}}
```

See §1.2. `Hello { protocol_version: u32 }` is the only field on the
handshake payload.

### 3.6 `PointerEvent(PointerEvent)`

```json
{
  "PointerEvent": {
    "event_type": "pointermove",
    "pointer_id": 1,
    "timestamp": 99999,
    "is_primary": true,
    "pointer_type": "pen",
    "button": 0,
    "buttons": 1,
    "x": 0.5,
    "y": 0.5,
    "pressure": 0.8,
    "tilt_x": -45,
    "tilt_y": 30,
    "twist": 90,
    "width": 0.0,
    "height": 0.0,
    "altitude_angle": 0.785,
    "azimuth_angle": 1.57
  }
}
```

A single pointer (mouse, pen, or touch) input event, forwarded 1:1 to the
platform input backend (`InputDevice::send_pointer_event`) as long as an
input device has been initialized by a prior `Config` message; otherwise it's
dropped with a warning log.

Fields (`PointerEvent` struct, `protocol.rs`):

| Field | Type | Notes |
|---|---|---|
| `event_type` | `PointerEventType` | one of `pointerdown`/`pointerup`/`pointercancel`/`pointermove`/`pointerover`/`pointerenter`/`pointerleave`/`pointerout` — mirrors the DOM PointerEvent types, serde-renamed to those exact lowercase strings |
| `pointer_id` | `i64` | browser's `PointerEvent.pointerId`, used to track multi-touch/multi-pen contacts |
| `timestamp` | `u64` | client-side event timestamp |
| `is_primary` | `bool` | DOM `isPrimary` |
| `pointer_type` | `PointerType` | `""` (Unknown), `"mouse"`, `"pen"`, or `"touch"` |
| `button` | `Button` (bitflags, wire: `u8`) | the button that changed state for this event |
| `buttons` | `Button` (bitflags, wire: `u8`) | all buttons currently held |
| `x`, `y` | `f64` | 0.0–1.0, relative to the capture area |
| `pressure` | `f64` | 0.0–1.0; 0.0 means no pressure data |
| `tilt_x`, `tilt_y` | `i32` | -90..90 degrees |
| `twist` | `i32` | stylus barrel rotation, degrees |
| `width`, `height` | `f64` | touch contact size, relative to capture area |
| `altitude_angle`, `azimuth_angle` | `Option<f32>` | optional stylus angle data (defaults to absent/`null` on older browsers — `#[serde(default)]`) |

`Button` is a bitflags `u8`: `NONE=0`, `PRIMARY=1`, `SECONDARY=2`,
`AUXILARY=4`, `FOURTH=8`, `FIFTH=16`, `ERASER=32`. On the wire it is a plain
integer (bitmask), not a string; an integer with any undefined bit set fails
to deserialize.

### 3.7 `BatchedPointerEvents(Vec<PointerEvent>)` (wire key: `batched_pointer_events`)

```json
{"batched_pointer_events": [ { "...": "PointerEvent" }, { "...": "PointerEvent" } ]}
```

A batch of `PointerEvent`s sent as a single WebSocket text frame instead of
one frame per event, to reduce per-frame overhead for input sources that
generate events faster than one-per-frame (e.g. high-frequency stylus/touch
sampling). The server applies each contained `PointerEvent` in order via the
same per-event path as §3.6. Note the wire key is lowercase
`batched_pointer_events` (explicit serde rename), unlike every other variant
which uses its Rust name verbatim.

### 3.8 `WheelEvent(WheelEvent)`

```json
{"WheelEvent": {"dx": 0, "dy": -120, "timestamp": 5000}}
```

Scroll-wheel input, forwarded to `InputDevice::send_wheel_event`. Fields:
`dx`, `dy` (`i32`, wheel delta) and `timestamp` (`u64`).

### 3.9 `KeyboardEvent(KeyboardEvent)`

```json
{
  "KeyboardEvent": {
    "event_type": "down",
    "code": "KeyA",
    "key": "a",
    "location": 0,
    "alt": false,
    "ctrl": true,
    "shift": false,
    "meta": false
  }
}
```

A single keyboard event, forwarded to `InputDevice::send_keyboard_event`.
Fields:

- `event_type`: `KeyboardEventType` — `"down"`, `"up"`, or `"repeat"`.
- `code`, `key`: strings mirroring the DOM `KeyboardEvent.code`/`.key`
  (physical key vs. logical character).
- `location`: `KeyboardLocation` (`STANDARD`/`LEFT`/`RIGHT`/`NUMPAD`), but on
  the wire this is sent as a **numeric code** (0/1/2/3), not a string — it
  uses a custom deserializer (`location_from`) that reads a `u8` and maps it;
  any other integer fails to deserialize.
- `alt`, `ctrl`, `shift`, `meta`: `bool` modifier state.

### 3.10 `GetCapturableList`

```json
"GetCapturableList"
```

Asks the server to (re-)enumerate capturable windows/screens on the host and
reply with `CapturableList` (§4.2.2). Sent once on every connect/reconnect,
and again whenever the user asks to refresh the list in the client UI (e.g.
after granting Wayland portal screen-share access, which can only be detected
by re-enumerating).

### 3.11 `Config(ClientConfiguration)`

```json
{
  "Config": {
    "uinput_support": true,
    "capturable_id": 0,
    "capture_cursor": true,
    "max_width": 1920,
    "max_height": 1080,
    "client_name": "my-tablet",
    "frame_rate": 30.0
  }
}
```

Configures (or reconfigures) capture, input, and encoding for this session.
Fields (`ClientConfiguration` struct):

- `uinput_support` (`bool`, Linux-only field, `#[cfg(target_os = "linux")]`):
  whether the client wants the server to use a `uinput` virtual input device
  rather than the enigo (XTest/synthetic) backend.
- `capturable_id` (`usize`): index into the most recently sent
  `CapturableList`, selecting which window/screen to capture.
- `capture_cursor` (`bool`): whether to composite the host cursor into the
  captured video.
- `max_width`, `max_height` (`usize`): client's requested maximum video
  resolution.
- `client_name` (`Option<String>`): identifies this client, used to decide
  whether to recreate the input device when it changes.
- `frame_rate` (`f64`): requested capture/encode frame rate.

Server behavior on receipt (`update_config`, `session.rs`): resolves
`capturable_id` against the server's current capturable list; if valid,
(re)creates the appropriate platform input device (uinput/enigo/Windows
input, depending on OS and `uinput_support`) unless an equivalent device
already exists, then sends `VideoCommands::Start` to the video thread with
the resolved capturable, cursor, resolution, and frame-rate settings — which
eventually surfaces as `ConfigOk` or an error (§4.2.4–4.2.6) once capture
actually starts or fails. If `capturable_id` is out of range (e.g. stale
after a list refresh), the server replies immediately with `ConfigError`
instead of touching the video thread.

The client resends `Config` on every reconnect and whenever local settings
(resolution, chosen window, frame rate, cursor capture, etc.) change.

### 3.12 `PauseVideo` / `ResumeVideo` / `RestartVideo`

```json
"PauseVideo"
"ResumeVideo"
"RestartVideo"
```

Forwarded verbatim to the video thread as `VideoCommands::Pause` / `::Resume`
/ `::Restart`. The reference client sends `PauseVideo` when the tab becomes
hidden (`document.hidden`) or when the user disables video in settings, and
`ResumeVideo` (followed by `RequestKeyframe`, §3.3) when the tab becomes
visible again and video is enabled. `RestartVideo` forces the video pipeline
to tear down and restart from scratch (used, e.g., after certain client-side
decode errors that a keyframe alone can't recover from).

### 3.13 `ChooseCustomInputAreas`

```json
"ChooseCustomInputAreas"
```

Only meaningful when the server binary is built with the `gui` feature.
Triggers the server's native GUI to let the user interactively draw custom
input rectangles (see `CustomInputAreas`/`Rect`, §4.2.7) for mouse/touch/pen,
which are then pushed back to the client as `MessageOutbound::CustomInputAreas`.
Without the `gui` feature, the server replies with
`MessageOutbound::Error("Custom input areas not available without GUI.")`
instead.

### 3.14 `BufferHealth(BufferHealth)`

```json
{"BufferHealth": {"buffer_seconds": 0.42}}
```

```rust
pub struct BufferHealth {
    /// Seconds of buffered video the client currently holds.
    pub buffer_seconds: f64,
}
```

The client reports how many seconds of decoded/buffered video it is
currently holding, sampled roughly every 30 video frames
(`health_frame_count >= 30` in `ts/lib.ts`) — either from the MSE
`SourceBuffer`'s buffered range minus current playback time, or a fixed
`0.05` placeholder when the WebCodecs low-latency path is active (which has
no MSE buffer; the decoder is fed live). The server forwards this to the
video thread as `VideoCommands::BufferHealth`, feeding the same
latency-budget/rate-control logic that `ClientRtt` feeds — buffer growth is a
signal the server is outrunning what the client can consume.

## 4. `MessageOutbound` — server → client

All variants below are declared on `MessageOutbound` in `protocol.rs`.

### 4.1 `Hello(Hello)`

See §1.2. Same shape as inbound `Hello`: `{"Hello": {"protocol_version": 3}}`.

### 4.2 `HelloNack { server_version, min_client_version, reason }`

See §1.3 for the full version-guard specification.

```rust
/// Sent when the client's protocol version is below
/// `MIN_CLIENT_PROTOCOL_VERSION`. Client should reload to fetch the
/// current bundle; keeping this as a typed message (not generic Error)
/// lets the client special-case the reload prompt.
HelloNack {
    server_version: u32,
    min_client_version: u32,
    reason: String,
},
```

### 4.2.1 `CapturableList(Vec<String>)`

```json
{"CapturableList": ["Screen 0", "Window — Firefox"]}
```

Sent in response to `GetCapturableList`. A list of human-readable names for
capturable windows/screens on the host, in the same order the server holds
them internally — the index into this list is what the client must send back
as `capturable_id` in `Config` (§3.11). May be empty (e.g. before the user
has granted screen-capture portal access on Wayland).

### 4.2.2 `NewVideo`

```json
"NewVideo"
```

Signals the start of a new video stream (e.g. after a capture restart, a
resolution/capturable change, or reconnect). The client tears down its old
`MediaSource`/`SourceBuffer` (or resets the WebCodecs pipe if active) and
prepares a fresh MSE session, expecting a `VideoInit` and then a run of
binary video frames to follow.

### 4.2.3 `VideoInit { codec_string }`

```json
{"VideoInit": {"codec_string": "avc1.4D4028"}}
```

```rust
/// Sent before video data to tell the client which codec string to use
/// for MSE addSourceBuffer (e.g. "avc1.4D4028" for Main Profile Level 4.0).
VideoInit {
    codec_string: String,
},
```

Sent once per `NewVideo` cycle, before the first binary video frame. Gives
the client the exact codec string to pass to
`MediaSource.addSourceBuffer('video/mp4; codecs="..."')`. The reference
client also uses this as the trigger to probe WebCodecs support for that
codec string and, if available and not already decided for this session,
switch to the lower-latency WebCodecs decode path instead of MSE — sending a
`RequestKeyframe` (§3.3) once the WebCodecs pipe is wired up, since it has no
prior frames to reference.

### 4.2.4 `ConfigOk`

```json
"ConfigOk"
```

Sent once capture/encode has actually started successfully after a `Config`
message (asynchronously — capture setup happens on the video thread, so this
can arrive after some delay, not synchronously with `Config`). The client
uses this to update its UI (e.g. clear a "connecting" state) and, on a
sourceBuffer error with a debounce guard, as the recovery hook it invokes by
resending its `Config` (`sourceBuffer.onerror` in `ts/lib.ts`).

### 4.2.5 `CustomInputAreas(CustomInputAreas)`

```json
{
  "CustomInputAreas": {
    "mouse": {"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5},
    "touch": null,
    "pen": {"x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0}
  }
}
```

Sent after `ChooseCustomInputAreas` (§3.13) once the user has interactively
picked input regions via the server's native GUI. `CustomInputAreas` holds an
optional `Rect { x, y, w, h }` per pointer type (`mouse`, `touch`, `pen`);
`Rect` defaults to the full capture area (`x: 0.0, y: 0.0, w: 1.0, h: 1.0`)
and `CustomInputAreas` defaults to all three areas absent (`None`). The
client persists the received value into its local settings and enables the
corresponding checkbox.

### 4.2.6 `ConfigError(String)` and `Error(String)`

```json
{"ConfigError": "No capturable selected. Click \"Refresh List\" to request screen access."}
{"Error": "Failed to read message!"}
```

Both carry a free-form human-readable string. `ConfigError` is specifically
about a `Config` message failing to apply (invalid `capturable_id`, failure
to create the platform input device) and is shown by the client via a
dedicated config-error UI path (`onConfigError`). `Error` is the general
catch-all — sent, for example, when the server fails to parse or otherwise
process a message it did successfully receive as a WebSocket text frame, or
when video-thread setup fails outside the `Config`-specific path — and is
shown as a generic toast.

### 4.2.7 `HeartbeatAck { server_ts_ms }`

See §3.1 for the full heartbeat/RTT specification.

## 5. Supporting types (not top-level messages)

These types appear only nested inside the messages above; they have no
standalone wire representation of their own.

- **`Rect { x, y, w, h }`** (`f64` each): a normalized rectangle within the
  capture area, `0.0..1.0`. Default is the full area (`0,0,1,1`). Used inside
  `CustomInputAreas`.
- **`CustomInputAreas { mouse, touch, pen }`**: three optional `Rect`s, one
  per pointer type. Used inside `MessageOutbound::CustomInputAreas`.
- **`PointerType`**: `Unknown` (wire: `""`), `Mouse` (`"mouse"`), `Pen`
  (`"pen"`), `Touch` (`"touch"`).
- **`PointerEventType`**: `DOWN`/`UP`/`CANCEL`/`MOVE`/`OVER`/`ENTER`/`LEAVE`/`OUT`,
  serialized as the corresponding DOM event names (`pointerdown`,
  `pointerup`, `pointercancel`, `pointermove`, `pointerover`, `pointerenter`,
  `pointerleave`, `pointerout`).
- **`KeyboardEventType`**: `DOWN`/`UP`/`REPEAT`, serialized as `"down"`,
  `"up"`, `"repeat"`.
- **`KeyboardLocation`**: `STANDARD`/`LEFT`/`RIGHT`/`NUMPAD`, serialized as a
  numeric code `0`/`1`/`2`/`3` via a custom deserializer — any other integer
  is a deserialization error.
- **`Button`**: a `bitflags` `u8` (`NONE`, `PRIMARY`, `SECONDARY`,
  `AUXILARY`, `FOURTH`, `FIFTH`, `ERASER`), serialized as a plain integer
  bitmask; an integer with an undefined bit set is a deserialization error.

## 6. Versioning and evolution

To add a new message type or field in a backward-compatible way, bump
`PROTOCOL_VERSION` but leave `MIN_CLIENT_PROTOCOL_VERSION` unchanged — old
clients keep working, new clients/servers can negotiate the new version. To
make a wire-breaking change (removing/renaming a variant, changing a field's
type or required-ness in a way an old client would misinterpret), bump both
`PROTOCOL_VERSION` and `MIN_CLIENT_PROTOCOL_VERSION` together so old clients
are refused via `HelloNack` (§1.3) instead of silently desyncing. There is no
per-message version field; version compatibility is gated once, at
handshake time, for the whole connection.
