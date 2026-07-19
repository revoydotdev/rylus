# Rylus — Vision

Rylus turns a tablet or smartphone into a pressure- and tilt-sensitive graphics
tablet and touch screen for a desktop computer, using only the tablet's browser.
It is a pure-Rust fork of [Weylus](https://github.com/H-M-H/Weylus): a native
capture/encode/input **server** on the PC paired with a zero-install browser
**PWA** on the tablet, connected over the local network.

This document is the project's constitution. The axioms below are durable
principles. Every Architecture Decision Record (`docs/adr/`) must justify itself
against them; when a decision contradicts an axiom, either the decision is wrong
or the axiom must be superseded by a new, explicit ADR — never drifted past
silently.

---

## Axioms

### AX-1 — Perceived stylus latency is the product
The single most important metric is end-to-end latency from pointer contact on
the tablet to the corresponding photon on the mirrored screen. Roughly 7 ms of
added latency is perceptible while inking (CHI 2014). Every change — capture,
encode, transport, decode, input injection — is weighed against its effect on
that budget *before* its effect on features, code size, or convenience. A
feature that regresses latency must earn its place explicitly.

### AX-2 — The zero-install browser client is sacred
The tablet runs nothing but a modern browser. There is no native tablet app, no
app-store gatekeeper, no SDK to sideload. Any capability that would require
installing, signing, or updating software *on the tablet* is out of scope. The
web client may be a PWA (installable to the home screen, offline shell cache)
but must remain a plain web page first.

### AX-3 — LAN-only, WebSocket-only transport (v1 contract)
v1 streams over a single WebSocket on the user's local network. There is no
cloud relay, no rendezvous server, no account, and no telemetry phone-home.
On this topology, Wi-Fi access-point contention dominates tail latency, so the
complexity of UDP/tiled/custom transports buys little in v1 (see
`docs/adr/ADR-0003-lan-only-websocket-transport-v1.md`). Changing the transport
requires a superseding ADR grounded in measured latency, not preference.

### AX-4 — One-time value, never a subscription or a gate
Rylus is bring-your-own-device software sold (or given) once. No feature is ever
premium-gated behind recurring pricing, and no capability depends on paid
cloud infrastructure that adds no real user value. The product bet is that
incumbency — not genuine infrastructure — is the moat of the paid competitors;
Rylus answers with parity-or-better quality at a fraction of the lifetime cost.

### AX-5 — Pure Rust, memory-safe end to end
Rylus exposes a network service on someone's LAN, so the whole codebase must be
auditable. No custom C or C++ is carried in the project. Every `unsafe` block
documents its invariants with a `// SAFETY:` comment. Foreign-function surfaces
(FFmpeg, platform APIs) are wrapped behind safe Rust with real error
propagation rather than panics.

### AX-6 — Secure by default for a network-exposed surface
Safe defaults are the shipped defaults: TLS on by default, access codes hashed
with argon2 and verified in constant time, per-IP rate limiting, bounded control
frames, idle-connection teardown, and explicit WebSocket `Origin` validation.
Weakening any of these is a deliberate, loudly-warned opt-out — never a silent
default. A safeguard that is documented but not wired is treated as a defect.

### AX-7 — Graceful degradation over panics
Real capture pipelines fail: hardware sleeps, compositors revoke portal access,
links drop. The system recovers — reconnect with backoff, capture auto-teardown,
keyframe-on-demand, MSE/decoder restart — rather than hanging, spinning, or
crashing. A user-visible failure is always a bounded, explained state, never an
indefinite freeze.

### AX-8 — Cross-platform parity without platform lock-in
Linux (X11 + Wayland), macOS, and Windows each get real capture, hardware-
accelerated encode, and input backends behind shared traits — no stubbed
platform. The browser client behaves identically everywhere. Each desktop
platform ships a trustworthy, signed installer appropriate to its ecosystem.

---

## Done at 1.0.0

Rylus is 1.0.0 when all of the following hold:

- **Latency is measured, not asserted.** A pointer-to-photon latency budget is
  instrumented and a checked-in baseline exists, with a CI regression gate on
  the encode path (AX-1).
- **The wire protocol is documented.** `docs/PROTOCOL.md` describes message
  types, framing, heartbeat v3, keyframe-on-demand, and RTT sampling, and
  matches `rylus-core::protocol` (AX-3, AX-7).
- **Security is reviewed and enforced.** A security review since 0.15.0 is
  written down (`docs/SECURITY-REVIEW.md`), and every claimed safeguard —
  including WebSocket `Origin` validation — is proven wired by a test (AX-6).
- **Every platform ships a signed, first-run-clean installer:** AUR
  (`rylus` + `rylus-bin`), a notarized universal macOS DMG, and a signed
  Windows MSI that runs on a clean machine (AX-8).
- **A user can reach it end to end** on each OS from the README quickstart
  without editing source, and a `--self-test` boot→capture→encode→WS path gates
  releases per-OS in CI (AX-2, AX-7).
- **Accessibility passes** WCAG 2.1 AA on the client routes with keyboard-only
  settings verified (AX-2).
- **Versioning is coherent:** a single `1.0.0` tag with a matching `CHANGELOG.md`
  entry, and the 0.7–0.17 tag-history question resolved on the record.

Everything risky and complex (the pure-Rust rewrite, four real platform
backends, the security layer, the low-latency pipeline) is already done. 1.0.0
is the deterministic, verifiable finishing pass.
</content>
</invoke>
