# Rylus Wire Protocol

This document specifies the Rylus wire protocol at **protocol version 3**. The
primary source of truth is `crates/rylus-core/src/protocol.rs`; this document
is derived from that file plus the transport, server, and TypeScript client
sources. All section headings cite the relevant source files.

---

## Transport

**Source:** `crates/rylus-transport/src/websocket.rs`, `crates/rylus-server/src/web.rs`

Rylus uses a single WebSocket connection for both control messages (JSON text
frames) and video data (binary frames). The server exposes the WebSocket
endpoint at:

```
ws[s]://<host>:<port>/ws
```

The scheme matches the page protocol — the client selects `ws://` for plain
HTTP and `wss://` for HTTPS connections (`ts/lib.ts`, lines 1396–1399).

### HTTP Upgrade

The upgrade follows RFC 6455: the server validates `Sec-WebSocket-Version: 13`
(else `426 Upgrade Required`) and `Sec-WebSocket-Key` (else `400`), and
responds `101 Switching Protocols` with `Upgrade: websocket`,
`Connection: Upgrade`, and the derived `Sec-WebSocket-Accept` header
(`ws_handshake_response`, `crates/rylus-server/src/web.rs`). Authentication
(session cookie) and the Origin check run before the handshake response is
built; a missing `Origin` header is accepted for non-browser clients.

Inbound messages are capped at 64 KiB at the protocol level
(`max_message_size`/`max_frame_size`); the video direction is outbound-only
and unaffected.

### TLS

**Source:** `crates/rylus-server/src/tls.rs`, `crates/rylus-server/src/web.rs`

TLS is optional. When enabled, the server uses `rustls` with a certificate
loaded from disk or generated on first run by `rcgen`. The per-connection
stream is a `TlsOrTcpStream` union type — both variants are accepted on the
same port. For external TLS termination, see `rylus_tls.sh`.

### Idle Timeout

A connection with no inbound messages for **120 seconds** is closed by the
server (`IDLE_TIMEOUT`, `crates/rylus-transport/src/websocket.rs:27`). The
client heartbeat (5 s cadence, see §Heartbeat) keeps live connections well
inside this bound.

---

## Service Discovery (mDNS)

**Source:** `crates/rylus-server/src/mdns.rs`

The server advertises itself on the local network using the Multicast DNS
service type `_rylus._tcp.local.`. The TXT record carries a single property:

```
version=3
```

The instance name is `Rylus <hostname> <4-char suffix>`. The suffix is derived
from PID and process start time to avoid collisions when multiple Rylus
instances run on the same LAN.

---

## Frame Types and Size Limits

**Source:** `crates/rylus-transport/src/websocket.rs`

| Frame type | Content | Size limit |
|------------|---------|------------|
| Text | Control messages (JSON) | **64 KB** (`MAX_TEXT_FRAME_SIZE = 64 * 1024`) |
| Binary | Video data (fMP4 segments) | None |

Inbound text frames exceeding 64 KB are dropped with a warning and never
forwarded to the session handler. The server ignores inbound binary frames,
Ping, Pong, and raw Frame messages from the client.

Outbound video frames are queued in a drop-oldest ring buffer of capacity
**4** (`VIDEO_QUEUE_CAPACITY`). When a slow client cannot drain the queue,
the oldest frames are silently dropped to keep the stream live-adjacent.
`WsRylusSender::dropped_video_frames()` exposes a cumulative drop counter for
telemetry.

---

## Compression (optional feature)

**Source:** `crates/rylus-transport/src/compress.rs`

The `rylus-transport` crate has a `compression` Cargo feature (disabled by
default) that wraps text message payloads in application-level raw deflate
(RFC 1951 via `flate2`). A compressed frame is prefixed with the byte `0x01`,
which is not a valid JSON start byte. Binary video frames are never compressed.

The client must detect this prefix before JSON parsing. The feature is listed
as a migration target toward `permessage-deflate` (RFC 7692) in a comment at
`crates/rylus-transport/src/compress.rs:5`.

---

## Message Encoding

**Source:** `crates/rylus-core/src/protocol.rs`

Control messages are serialized with `serde_json` using Rust's default
**externally tagged** enum representation:

- **Unit variants** serialize as a bare JSON string: `"Heartbeat"`
- **Newtype variants** serialize as a single-key object:
  `{"Hello": {"protocol_version": 3}}`
- **Struct variants** serialize as a single-key object with a fields object:
  `{"HeartbeatAck": {"server_ts_ms": 1712345678000}}`

One rename exception: `BatchedPointerEvents` is serialized as
`"batched_pointer_events"` (lowercase, `#[serde(rename = "batched_pointer_events")]`).

---

## Protocol Version and Handshake

**Source:** `crates/rylus-core/src/protocol.rs`, `crates/rylus-server/src/session.rs`

```
PROTOCOL_VERSION         = 3   // current server version
MIN_CLIENT_PROTOCOL_VERSION = 2   // oldest client the server will accept
```

**Handshake sequence:**

1. Client opens the WebSocket connection.
2. Client immediately sends `Hello` with its own version:

   ```json
   {"Hello": {"protocol_version": 3}}
   ```

3. Server evaluates the client's version:
   - If `client.protocol_version < MIN_CLIENT_PROTOCOL_VERSION` (currently 2),
     the server sends `HelloNack` and the connection is unusable:

     ```json
     {
       "HelloNack": {
         "server_version": 3,
         "min_client_version": 2,
         "reason": "Client is too old — reload the page to pick up the new bundle."
       }
     }
     ```

   - Otherwise the server responds with its own version:

     ```json
     {"Hello": {"protocol_version": 3}}
     ```

4. The negotiated version is `min(client_version, server_version)`. Only
   features available in both endpoints should be used.

---

## Heartbeat

**Source:** `crates/rylus-transport/src/websocket.rs:27`, `crates/rylus-server/src/session.rs:178–187`, `ts/lib.ts:1459–1465`

The client sends a heartbeat every **5 seconds** (`HEARTBEAT_INTERVAL = 5000` ms).
Each heartbeat is a unit-variant JSON string:

```json
"Heartbeat"
```

The server echoes the receive timestamp so the client can measure round-trip
time:

```json
{"HeartbeatAck": {"server_ts_ms": 1712345678000}}
```

`server_ts_ms` is milliseconds since the Unix epoch.

On receiving `HeartbeatAck`, the client computes `RTT = now − heartbeat_send_time`
and sends it back as a `ClientRtt` message:

```json
{"ClientRtt": {"rtt_ms": 42}}
```

The server feeds this measurement into the adaptive quality controller (see
§Adaptive Quality). The client also drives a connection-quality indicator in the
UI from the RTT series.

---

## Inbound Messages (client → server)

**Source:** `crates/rylus-core/src/protocol.rs` (`MessageInbound` enum), `crates/rylus-server/src/session.rs`

| Message | JSON | Description |
|---------|------|-------------|
| `Hello` | `{"Hello": {"protocol_version": N}}` | Handshake (see §Handshake) |
| `Heartbeat` | `"Heartbeat"` | Keepalive; resets the 120 s idle timer |
| `ClientRtt` | `{"ClientRtt": {"rtt_ms": N}}` | RTT measurement derived from HeartbeatAck |
| `GetCapturableList` | `"GetCapturableList"` | Request list of capture sources |
| `Config` | `{"Config": {...}}` | Configure capture; triggers video start |
| `PointerEvent` | `{"PointerEvent": {...}}` | Single pointer event |
| `BatchedPointerEvents` | `{"batched_pointer_events": [...]}` | Array of pointer events |
| `WheelEvent` | `{"WheelEvent": {...}}` | Scroll event |
| `KeyboardEvent` | `{"KeyboardEvent": {...}}` | Key press/release |
| `PauseVideo` | `"PauseVideo"` | Pause the capture/encode loop |
| `ResumeVideo` | `"ResumeVideo"` | Resume the capture/encode loop |
| `RestartVideo` | `"RestartVideo"` | Force encoder restart |
| `BufferHealth` | `{"BufferHealth": {"buffer_seconds": N.N}}` | Client buffer depth report |
| `RequestKeyframe` | `"RequestKeyframe"` | Demand an IDR keyframe (see §Keyframe-on-Demand) |
| `ChooseCustomInputAreas` | `"ChooseCustomInputAreas"` | Open the server-side input area GUI |

### Config

```json
{
  "Config": {
    "capturable_id": 0,
    "capture_cursor": true,
    "max_width": 1920,
    "max_height": 1080,
    "frame_rate": 30.0,
    "client_name": "my-tablet",
    "uinput_support": true
  }
}
```

- `capturable_id`: index into the most recently received `CapturableList`.
- `uinput_support`: Linux-only field; absent on other platforms.
- `max_width` / `max_height`: encoder output ceiling in pixels.
- `frame_rate`: target capture rate in Hz (floating point).
- `client_name`: optional string shown in server logs and used as the uinput
  device name on Linux.

### PointerEvent

```json
{
  "PointerEvent": {
    "event_type": "pointermove",
    "pointer_id": 1,
    "timestamp": 1712345678000000,
    "is_primary": true,
    "pointer_type": "pen",
    "button": 0,
    "buttons": 1,
    "x": 0.5,
    "y": 0.5,
    "pressure": 0.8,
    "tilt_x": -15,
    "tilt_y": 30,
    "twist": 0,
    "width": 0.01,
    "height": 0.01,
    "altitude_angle": 0.785,
    "azimuth_angle": 1.57
  }
}
```

| Field | Type | Range / Notes |
|-------|------|---------------|
| `event_type` | string | `"pointerdown"`, `"pointerup"`, `"pointercancel"`, `"pointermove"`, `"pointerover"`, `"pointerenter"`, `"pointerleave"`, `"pointerout"` |
| `pointer_id` | i64 | Browser `PointerEvent.pointerId` |
| `timestamp` | u64 | Browser `PointerEvent.timeStamp × 1000` (microseconds; see note) |
| `is_primary` | bool | |
| `pointer_type` | string | `""`, `"mouse"`, `"pen"`, `"touch"` |
| `button` | u8 | Bitfield: PRIMARY=0x01, SECONDARY=0x02, AUXILIARY=0x04, FOURTH=0x08, FIFTH=0x10, ERASER=0x20 |
| `buttons` | u8 | Same bitfield, currently-held buttons |
| `x`, `y` | f64 | 0.0–1.0, normalized to the capture area |
| `pressure` | f64 | 0.0–1.0; 0.0 means no pressure data |
| `tilt_x`, `tilt_y` | i32 | −90 to 90 degrees |
| `twist` | i32 | 0–359 degrees |
| `width`, `height` | f64 | Touch contact size, normalized to the capture area diagonal |
| `altitude_angle` | f32? | Optional; absent when not reported by the browser |
| `azimuth_angle` | f32? | Optional; absent when not reported by the browser |

> **Timestamp note:** The TypeScript client computes
> `Math.round(event.timeStamp * 1000)` (`ts/lib.ts:557`). The browser's
> `PointerEvent.timeStamp` is in milliseconds from the time origin, so the
> server receives a microsecond-resolution value relative to page load, not
> Unix epoch. The server does not interpret this field directly; it is passed
> through to the uinput/input layer.

`BatchedPointerEvents` carries an array of these structs in the same field
layout, JSON key `"batched_pointer_events"` (lowercase).

### KeyboardEvent

```json
{
  "KeyboardEvent": {
    "event_type": "down",
    "code": "KeyA",
    "key": "a",
    "location": 0,
    "alt": false,
    "ctrl": false,
    "shift": false,
    "meta": false
  }
}
```

| Field | Type | Notes |
|-------|------|-------|
| `event_type` | string | `"down"`, `"up"`, `"repeat"` |
| `code` | string | W3C `KeyboardEvent.code` (e.g. `"KeyA"`, `"NumpadEnter"`) |
| `key` | string | W3C `KeyboardEvent.key` (printable or named key string) |
| `location` | u8 | 0=STANDARD, 1=LEFT, 2=RIGHT, 3=NUMPAD |
| `alt`, `ctrl`, `shift`, `meta` | bool | Modifier state |

### WheelEvent

```json
{
  "WheelEvent": {
    "dx": 0,
    "dy": 120,
    "timestamp": 1712345678000000
  }
}
```

`dx` and `dy` are integer scroll deltas in pixels. The client normalizes
browser `deltaMode` values to pixels before sending (`ts/lib.ts:619–628`).

---

## Outbound Messages (server → client)

**Source:** `crates/rylus-core/src/protocol.rs` (`MessageOutbound` enum), `crates/rylus-server/src/session.rs`

| Message | JSON | Description |
|---------|------|-------------|
| `Hello` | `{"Hello": {"protocol_version": 3}}` | Handshake response |
| `HelloNack` | `{"HelloNack": {"server_version": N, "min_client_version": N, "reason": "..."}}` | Version rejection |
| `CapturableList` | `{"CapturableList": ["Screen 0", "Window: Krita"]}` | Available capture sources |
| `ConfigOk` | `"ConfigOk"` | Config accepted; video capture starting |
| `ConfigError` | `{"ConfigError": "message"}` | Config rejected (invalid index, uinput failure, etc.) |
| `NewVideo` | `"NewVideo"` | Client must reset its MSE SourceBuffer |
| `VideoInit` | `{"VideoInit": {"codec_string": "avc1.4D4028"}}` | Codec string for MSE `addSourceBuffer` |
| `CustomInputAreas` | `{"CustomInputAreas": {...}}` | Input area rects from server GUI |
| `HeartbeatAck` | `{"HeartbeatAck": {"server_ts_ms": N}}` | Echo of client Heartbeat |
| `Error` | `{"Error": "message"}` | Generic server error |

Video data follows as binary WebSocket frames (see §Video Stream).

### CustomInputAreas

```json
{
  "CustomInputAreas": {
    "mouse": {"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5},
    "touch": null,
    "pen": {"x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0}
  }
}
```

Each `Rect` field (`mouse`, `touch`, `pen`) is either null or an object with
`x`, `y`, `w`, `h` in the range 0.0–1.0 (fraction of the canvas area). When
present, the client remaps pointer coordinates into that sub-region before
sending events.

---

## Video Stream

**Source:** `crates/rylus-server/src/session.rs`, `crates/rylus-encode/src/lib.rs` (referenced)

The video stream is H.264 encoded into fragmented MP4 (fMP4) segments for
browser Media Source Extensions. Segments arrive as binary WebSocket frames
immediately following the signaling sequence:

```
server → "NewVideo"
server → {"VideoInit": {"codec_string": "avc1.4D4028"}}
server → <binary fMP4 segment> (repeating)
```

The client must call `addSourceBuffer(codec_string)` before appending segment
data. `NewVideo` signals that the previous SourceBuffer is invalidated and a
new one is needed (encoder restart, resolution change, reconnect).

Video frames are dispatched with drop-oldest semantics (queue depth 4): a slow
client loses old frames rather than stalling the encoder. The client's
`BufferHealth` report and `RequestKeyframe` mechanism allow it to recover.

---

## Keyframe-on-Demand

**Source:** `crates/rylus-server/src/session.rs:167–169`, `crates/rylus-core/src/protocol.rs:56–59`

The client can request an IDR (intra-coded) keyframe at any time:

```json
"RequestKeyframe"
```

The server forwards this to the encode thread, which forces an IDR on the next
captured frame. This allows the client to recover a clean decoder state without
waiting for the next natural GOP boundary. Typical trigger conditions:

- WebSocket reconnect
- Browser tab regaining focus
- MSE decode error

---

## Adaptive Quality

**Source:** `crates/rylus-server/src/session.rs` (`QualityController`)

The server adjusts the encoder QP (quantization parameter) using two
independent signals:

| Signal | Source | Direction |
|--------|--------|-----------|
| Buffer health | `BufferHealth.buffer_seconds` from client | High buffer → raise QP (reduce quality) |
| Pipeline ratio | Encode time vs. frame budget (server-local) | Encoder saturated → raise QP |

QP range: 18 (best) to 45 (worst), default 23. Buffer health wins when the two
signals disagree. When the client reports RTT > 80 ms via `ClientRtt`, the
controller tightens its thresholds to account for the reduced latency budget.

---

## Known Gaps and Uncertainties

- **`timestamp` units**: The server struct declares `timestamp: u64` with no
  unit comment (`protocol.rs:234`). The TypeScript client sends microseconds
  relative to page load (not Unix epoch). Whether the input backend
  (`rylus-input`) interprets this value or ignores it is not documented in
  the sources read; treat this field as opaque until `rylus-input` is audited.

- **`BufferHealth` cadence**: The client code sends `BufferHealth` on buffer
  update events, but the exact trigger interval is not visible in the sources
  read here; it depends on MSE `SourceBuffer` event timing in the browser.

- **Compression feature**: The `compression` Cargo feature (`compress.rs`) is
  not yet wired end-to-end in the transport layer per the TODO comment at line
  5. Client-side decompression logic is not present in `ts/lib.ts`. This
  feature is not active in the v0.17.0 release.

- **Multi-client broadcast (`StreamSession`)**: `session.rs` contains a
  `StreamSession` / `video_forwarder` pair that shares one capture+encode
  pipeline across multiple WebSocket clients via a broadcast channel. This
  struct is marked `#[allow(dead_code)]` and is not wired into the server's
  connection handler; it is not part of the current protocol surface.
