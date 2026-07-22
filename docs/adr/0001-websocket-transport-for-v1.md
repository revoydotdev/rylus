# ADR-0001: WebSocket (TCP) transport for v1 — no UDP/tiled protocol

**Status:** Accepted (2026-07-14)

## Context

Rylus streams H.264/fMP4 over a single WebSocket and receives input events on
the same connection. The market leader (Astropad) attributes its latency edge
to LIQUID, a proprietary UDP protocol that codes frames as independent tiles
so packet loss corrupts one tile rather than stalling a GOP. The obvious
question for a competitor chasing SOTA latency: should Rylus abandon
WebSocket for a custom UDP/tiled transport (or WebRTC DataChannels)?

A 2026 prior-art sweep (see the project's prior-art brief) found:

- No competitor publishes independently verified latency numbers. Astropad's
  "11.3 ms USB / 22.4 ms WiFi" and its unfavorable Duet comparisons are
  self-reported by Astropad.
- BLADE (arXiv:2603.16119) measured that on tablet-over-Wi-Fi-to-LAN-host
  topologies — exactly Rylus's deployment — **Wi-Fi access-point contention
  dominates tail latency**, not the transport protocol or encoder. The same
  congestion delays UDP and TCP packets alike on the last hop.
- The browser is Rylus's client contract (zero-install is the product's core
  differentiator). Browsers offer no raw UDP; the only standards path is
  WebRTC, which brings an ICE/DTLS/SCTP stack, certificate plumbing, and a
  materially larger failure surface for a LAN-only product.
- Perceptual thresholds (CHI 2014 stylus study): users discriminate ~7 ms
  differences while inking. The wins available from client-side techniques —
  predicted pointer events, keyframe-on-demand, drop-oldest send queues, a
  jitter-adaptive playout buffer — spend the same budget without a transport
  rewrite.

## Decision

Keep the single WebSocket (TCP, optional TLS) as the only transport for v1.
Invest the latency budget in: encoder low-delay flags, keyframe-on-demand,
drop-oldest video queueing, coalesced+predicted pointer events, and a
jitter-adaptive client playout buffer. Surface Wi-Fi quality to the user
(RTT/jitter indicator) instead of hiding the dominant latency source.

## Consequences

- A lost TCP segment stalls all video data behind it (head-of-line blocking)
  until retransmit; recovery relies on keyframe-on-demand plus the drop-oldest
  queue rather than tile-level resilience.
- The client remains a plain browser PWA with no install and no WebRTC
  permission/complexity cost.
- If profiling ever shows transport HOL blocking (not Wi-Fi contention) as
  the measured bottleneck, the revisit path is WebTransport/HTTP-3 (QUIC
  datagrams reach browsers without a custom native client) — not raw UDP.

## Roads not taken

- **Custom UDP + tiled coding (Astropad LIQUID style):** requires a native
  client app, abandoning the zero-install browser contract; benefit unproven
  for LAN once Wi-Fi contention is accounted for.
- **WebRTC:** standards-compliant UDP-ish path, but the ICE/DTLS/SCTP stack,
  self-signed-cert interplay, and connection-state machine add substantial
  complexity for a LAN-only, single-hop product. Deskreen (same AGPL license)
  demonstrates it works, but also demonstrates the complexity cost.
