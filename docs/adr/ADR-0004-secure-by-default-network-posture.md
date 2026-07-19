# ADR-0004 — Secure-by-default posture for a network-exposed service

- **Status:** Accepted
- **Date:** 2026-07-19
- **Relates to:** VISION.md AX-6, AX-7

## Context

Rylus runs an HTTP + WebSocket server on the user's LAN and, through the uinput /
platform input backends, can synthesize input system-wide. That is a high-value
target: an unauthenticated LAN peer that reaches the port could enumerate
screens, read configuration, or inject input. Upstream Weylus compared access
codes in plaintext, passed them as GET query parameters (leaking them into URLs,
logs, and browser history), and ran without transport encryption. For a tool
that a non-expert may leave running, the *default* configuration must be the
safe one, and safeguards must be genuinely wired rather than merely claimed.

## Decision

Rylus ships safe defaults and treats weakening any of them as a deliberate,
loudly-warned opt-out.

- **TLS on by default.** The server generates and serves a self-signed
  certificate on first run unless `--tls-mode disabled` is passed, which logs a
  loud `WARN`. Access codes and input events are not cleartext on the LAN by
  default.
- **Access-code authentication.** Codes are hashed with argon2 and verified in
  constant time; authentication is a POST body, never a query parameter.
- **Session management.** Authenticated sessions use `HttpOnly`,
  `SameSite=Strict` cookies; sessions are invalidated on access-code rotation.
- **Rate limiting.** Five failed attempts per 60-second window per IP trigger a
  30-second lockout.
- **Resource bounds.** WebSocket text frames are capped (64 KB) to prevent OOM
  from oversized control messages; an idle timeout closes zombie connections and
  frees video/encode/input resources.
- **Origin validation.** WebSocket upgrades are checked against the expected
  origin.
- **Authenticated surfaces.** `/settings`, `/api/config`, and the stream are all
  gated behind the access-code session.

A safeguard that is documented but not wired is treated as a defect
(VISION.md AX-6): every item above must be provable by a test. The M2 milestone
in `ROADMAP.md` exists specifically to verify the Origin check and to write down
the review in `docs/SECURITY-REVIEW.md`.

## Consequences

- The out-of-the-box configuration is defensible on an untrusted-adjacent LAN;
  users must consciously reduce security, and are warned when they do.
- Self-signed TLS produces browser certificate warnings, and a known Firefox bug
  requires accepting the WebSocket certificate separately; this friction is
  accepted as the cost of encryption-by-default and documented in `Readme.md`.
- Because "claimed but unwired" is a defect, security claims carry a standing
  obligation to back them with tests — hence M2.P1's Origin-enforcement test and
  the security-review document as 1.0.0 gates.
- Rylus does not attempt to be safe on a hostile network segment or against an
  on-path attacker beyond self-signed TLS; it is a LAN tool (ADR-0003), and that
  boundary is stated rather than implied.
</content>
