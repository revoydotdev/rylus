# Rylus — Roadmap to 1.0.0

Rylus is at **v0.17.0 (alpha)**. The core capture / encode / transport / input
stack is feature-complete and production-grade across all four platforms; the
path to 1.0 is packaging, distribution polish, documentation, and pre-release
hardening — not new features.

This is a public-facing roadmap. It is intentionally light on dates; milestones
land when they're ready.

---

## Already shipped (v0.17.0)

- Cargo workspace (7 crates), Rust 2021, TypeScript PWA via esbuild
- CI (`.github/workflows/build.yml`): `cargo test --workspace --locked`, vitest,
  clippy (`-D warnings`), rustfmt, cargo-audit, plus release builds for Linux
  (zip + `.deb` + Alpine/musl Docker), macOS (`cargo bundle`), and Windows (MSVC)
- Capture backends: X11 (x11rb + MIT-SHM), PipeWire/Wayland, CoreGraphics, Windows GDI
- Encode: FFmpeg H.264 → fMP4, keyframe-on-demand, HW paths (VAAPI/NVENC/VideoToolbox/MediaFoundation)
- Transport: WebSocket, optional self-signed TLS, mDNS discovery, heartbeat (proto v3)
- Input synthesis: uinput (Linux), WinRT (Windows), enigo (macOS)
- egui desktop GUI; argon2 access-code auth; per-IP rate limiting; 64 KB frame cap; 120 s idle timeout
- 200+ tests across Rust and TypeScript
- `--print-man-page` (clap_mangen); `--no-gui` headless mode
- Linux packaging: AUR `rylus` + `rylus-bin` PKGBUILDs (`.SRCINFO` auto-updated;
  `publish-aur` CI job on tag), systemd user unit, `.desktop` entry, multi-size
  icons from `packaging/icons/rylus.svg` via `scripts/gen-icons.sh`
- Architecture overview in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)

## Path to 1.0

> The CI release pipeline already **builds** macOS and Windows artifacts on tag.
> The gap below is signed/notarized *installers* and pre-release hardening, not
> the builds themselves.

### Distribution

- [x] **Linux — AUR packages** (`rylus`, `rylus-bin`), published on tag.
- [ ] **macOS — notarized universal DMG.** Extend the existing `cargo bundle`
      job to lipo x86_64 + arm64, sign with Developer ID, notarize via
      `notarytool`, staple, and package a DMG. Gated on the Developer ID cert.
- [ ] **Windows — signed MSI.** WiX v4 installer (`packaging/windows/rylus.wxs`,
      to be added) bundling required DLLs, firewall rules, Authenticode-signed.
      Smoke-tested on the Tiny11 CI runner. Gated on the Authenticode cert.

### Pre-release hardening

- [x] **`rylus-server --self-test` flag.** Boot → capture one frame → encode one
      frame → accept one WebSocket client → exit clean. Wired into a per-OS CI
      smoke matrix that gates releases.
- [ ] **Performance budget.** Latency targets per backend (capture → encode →
      wire); benchmark harness under `crates/rylus-encode/benches/`; CI compares
      to a checked-in baseline and fails on regression beyond a defined threshold.
- [ ] **PWA accessibility audit.** axe-core across all client routes; WCAG 2.1 AA
      findings fixed or filed with rationale; keyboard-only settings verified.
- [x] **Security review.** Diff review since 0.15.0 — argon2 params, rate-limit
      tuning, TLS cert generation, session-token lifecycle, explicit WebSocket
      `Origin` validation. Findings in `docs/SECURITY-REVIEW.md`.

### 1.0 release

- [x] **`docs/PROTOCOL.md`** describing the wire format (message types, framing,
      heartbeat, keyframe-on-demand).
- [ ] **Documentation pass.** README quickstart corrected against the current CLI
      and CHANGELOG entry added; cross-OS verification (macOS, Windows) still
      outstanding — cannot be completed from a Linux-only build environment.
- [ ] **Tag v1.0.0.** Signed annotated tag (note: current git history is tagged
      only through v0.6.1 — annotate the 0.7–0.17 line or start clean at 1.0);
      GitHub release with packaging artifacts; AUR `rylus-bin` updated shortly after.

---

## Out of scope (deliberate)

- Native iOS / Android apps — the browser PWA is the only client we ship.
- WebRTC / DataChannel — WebSocket transport is the v1 contract.
- Cloud relay or NAT traversal — Rylus is LAN-only for 1.0.
- Cursor warping or screen-space remap beyond what 0.17.0 ships.

## Contributing to the roadmap

Open an issue to suggest a milestone change, or a pull request for any item
above. See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## revoy ledger block

Machine-readable current phase for the revoy cross-project ledger. Mirrors the
"Path to 1.0" items above; keep in sync when items land.

<!-- revoy:begin -->
```toml
phase = "Path to 1.0"

[[todo]]
line = "PWA accessibility audit (axe-core all routes, WCAG 2.1 AA, keyboard-only settings)"
difficulty = 35
priority = "MED"

[[todo]]
line = "Documentation pass: README quickstart verified on all three OSes; CHANGELOG 1.0 entry"
difficulty = 20
priority = "MED"

[[todo]]
line = "Tag v1.0.0 (signed annotated tag, GitHub release with artifacts, AUR rylus-bin update)"
difficulty = 15
priority = "LOW"
```
<!-- revoy:end -->
