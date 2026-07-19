# ADR-0003 — LAN-only, WebSocket-only transport for v1

- **Status:** Accepted
- **Date:** 2026-07-19
- **Relates to:** VISION.md AX-1, AX-3, AX-4

## Context

Rylus streams a live H.264 screen mirror one way and pointer/keyboard input the
other, between a desktop and a tablet on the same local network. The dominant
product metric is perceived stylus latency (AX-1): roughly 7 ms of added latency
is perceptible while inking. It is tempting to reach for a custom UDP protocol,
tiled/partial-frame encoding, or a WebRTC data channel to shave transport
latency.

Two facts constrain the choice. First, the tablet client must remain a
zero-install browser page (AX-2), which rules out transports a browser cannot
speak without native code. Second, on the LAN topology Rylus targets, tail
latency is dominated by Wi-Fi access-point contention rather than by transport
framing overhead; the prior-art review (including the BLADE analysis of
Wi-Fi AP contention on this class of topology) found that UDP/tiled transports
buy little on a single-AP home network while adding substantial protocol,
reliability, and browser-compatibility complexity.

## Decision

v1 uses a single WebSocket over the LAN as the sole transport, carrying JSON
control frames and binary fragmented-MP4 video, played in the browser via Media
Source Extensions (with a WebCodecs decode path where available).

- No cloud relay, rendezvous server, account, or telemetry phone-home. Traffic
  stays on the user's network (AX-3, AX-4).
- Latency is attacked *inside* this contract — keyframe-on-demand
  (`RequestKeyframe`), RTT-aware rate control (`ClientRtt`/`HeartbeatAck`),
  drop-oldest video queuing, `TCP_NODELAY`, low-latency encoder tuning, and a
  WebCodecs decode path — rather than by changing the transport.
- Reliability lives in the protocol: heartbeat v3, reconnect with exponential
  backoff, capture auto-teardown, and MSE/decoder restart (AX-7).

### Alternatives rejected

- **Custom UDP / QUIC:** not reachable from an unmodified browser; would break
  AX-2. Latency upside is small on a contended single-AP LAN.
- **WebRTC:** browser-reachable, but adds ICE/SDP/STUN machinery and a signaling
  path for negligible benefit on a direct LAN, and complicates the security
  model.
- **Tiled/partial-frame encoding:** meaningful engineering cost for gains the
  prior-art review judged marginal against Wi-Fi contention on this topology.

## Consequences

- The transport surface stays small, browser-native, and auditable, keeping the
  zero-install client promise.
- The latency budget is spent on capture/encode/decode/pacing, which is where
  the measurable wins are on this topology; `docs/LATENCY.md` (M2) records the
  measured budget.
- If future targets change the topology (e.g. multi-AP, internet relay, or a
  measured case where transport framing dominates tail latency), this decision
  must be revisited via a **superseding ADR** grounded in measurement — not
  changed silently. This is the v1 contract, not a permanent ban on other
  transports.
</content>
