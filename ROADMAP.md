# Rylus — Roadmap

Decomposition of the path from the current state (`0.17.0` in `Cargo.toml`;
core stack production-grade; ~85% to 1.0.0) to the **1.0.0** release defined in
[`VISION.md`](VISION.md).

Scheme: milestones (`# M#`) → phases (`## M#.P#`) → stages (`### M#.P#.S#`) →
todos. Each todo is atomic and verifiable:

`- **`M#.P#.S#.T#`** — <task> → *Artifact:* <check that proves it, exit 0> · *Concern:* <tag>`

Every milestone ends with a `## M#.P9 — Milestone quality gates` section whose
`- **M#G#**` items each carry an objective check. Concern tags:
`latency` · `security` · `docs` · `testing` · `ci` · `packaging` · `a11y` ·
`reliability` · `release`.

External prerequisites (developer-program enrollment, code-signing cert
purchase, recurring costs) are tracked separately in `TODOS.md`; this roadmap
covers the engineering work the swarm can build and verify. Artifact checks are
run from the repository root.

---

# M1 — Release hardening & wire-protocol documentation

No external prerequisites. Immediately buildable — this is the first tick's
work. Delivers the release self-test, the wire-format spec, and an encode
benchmark baseline with a CI regression gate (AX-1, AX-3, AX-7).

## M1.P1 — `--self-test` boot path

### M1.P1.S1 — CLI flag & routine

- **`M1.P1.S1.T1`** — Add a `--self-test` flag to the `rylus-server` clap CLI (headless, mutually usable with `--no-gui`). → *Artifact:* `cargo run -q -p rylus-server -- --help | grep -q -- '--self-test'` · *Concern:* reliability
- **`M1.P1.S1.T2`** — Implement the routine: boot → open the `testsrc` capturable → encode one GOP → bind and accept a loopback WebSocket → exit `0` on success, non-zero on any stage failure. Use `testsrc` so it needs no display or hardware. → *Artifact:* `cargo run -q -p rylus-server -- --self-test` (exit 0) · *Concern:* reliability
- **`M1.P1.S1.T3`** — Add an integration test that invokes the self-test path and asserts a clean exit and teardown (no leaked threads/devices). → *Artifact:* `cargo test -p rylus-server self_test` · *Concern:* testing

### M1.P1.S2 — CI smoke matrix

- **`M1.P1.S2.T1`** — Add a `--self-test` step to the Linux CI job in `.github/workflows/build.yml`, gating the build. → *Artifact:* `grep -q 'self-test' .github/workflows/build.yml` · *Concern:* ci

## M1.P2 — Wire-protocol documentation

### M1.P2.S1 — docs/PROTOCOL.md

- **`M1.P2.S1.T1`** — Write `docs/PROTOCOL.md` documenting connection handshake, framing (text control frames vs. binary fMP4 video), and every `MessageInbound`/`MessageOutbound` variant in `crates/rylus-core/src/protocol.rs`. → *Artifact:* `test -f docs/PROTOCOL.md && grep -q 'MessageInbound' docs/PROTOCOL.md && grep -q 'MessageOutbound' docs/PROTOCOL.md` · *Concern:* docs
- **`M1.P2.S1.T2`** — Document heartbeat v3 (`Heartbeat`/`HeartbeatAck`, 5s interval, RTT sampling), keyframe-on-demand (`RequestKeyframe`), `ClientRtt`, and the `HelloNack` version-guard, stating `PROTOCOL_VERSION = 3` and `MIN_CLIENT_PROTOCOL_VERSION = 2`. → *Artifact:* `grep -q 'HeartbeatAck' docs/PROTOCOL.md && grep -q 'RequestKeyframe' docs/PROTOCOL.md && grep -q 'HelloNack' docs/PROTOCOL.md` · *Concern:* docs
- **`M1.P2.S1.T3`** — Add a test asserting the documented protocol version matches the constant in code, so the doc cannot silently drift. → *Artifact:* `cargo test -p rylus-core protocol_version` · *Concern:* testing

## M1.P3 — Encode benchmark harness & baseline

### M1.P3.S1 — Criterion benches

- **`M1.P3.S1.T1`** — Add `crates/rylus-encode/benches/` with a criterion benchmark over the encode hot path on a fixed synthetic frame source. → *Artifact:* `cargo bench -p rylus-encode --no-run` · *Concern:* latency
- **`M1.P3.S1.T2`** — Check in a baseline (`crates/rylus-encode/benches/BASELINE.md` or `.json`) recording measured per-frame encode timings and the host they were taken on. → *Artifact:* `ls crates/rylus-encode/benches/BASELINE.* ` · *Concern:* latency

### M1.P3.S2 — CI regression gate

- **`M1.P3.S2.T1`** — Wire a benchmark step into `.github/workflows/build.yml` that fails on a regression beyond a stated threshold versus the baseline. → *Artifact:* `grep -q 'bench' .github/workflows/build.yml` · *Concern:* ci

## M1.P9 — Milestone quality gates

- **M1G1** — Workspace builds and all tests pass. → *Check:* `cargo test --workspace --locked`
- **M1G2** — No clippy warnings. → *Check:* `cargo clippy --all-targets -- -D warnings`
- **M1G3** — Formatting clean. → *Check:* `cargo fmt -- --check`
- **M1G4** — Headless self-test exits 0. → *Check:* `cargo run -q -p rylus-server -- --self-test`
- **M1G5** — Protocol spec exists and covers v3 messages. → *Check:* `test -f docs/PROTOCOL.md && grep -q 'HeartbeatAck' docs/PROTOCOL.md`

---

# M2 — Security review & latency verification

Prove the shipped safeguards are actually wired, write down the security
posture, and turn "latency is the product" from an assertion into a measurement
(AX-1, AX-6). No external prerequisites.

## M2.P1 — Security review since 0.15.0

### M2.P1.S1 — Verify WebSocket Origin enforcement

- **`M2.P1.S1.T1`** — Audit whether commit `617c661`'s WebSocket `Origin` check is actually enforced on the upgrade path in `rylus-server`/`rylus-transport`; enforce it if not. → *Artifact:* `grep -rniq 'origin' crates/rylus-server/src crates/rylus-transport/src` · *Concern:* security
- **`M2.P1.S1.T2`** — Add a test proving a WebSocket upgrade with a foreign/absent `Origin` is rejected and a same-origin upgrade is accepted. → *Artifact:* `cargo test -p rylus-server origin` · *Concern:* testing

### M2.P1.S2 — Documented review

- **`M2.P1.S2.T1`** — Write `docs/SECURITY-REVIEW.md` covering argon2 parameters, rate-limit window/lockout, TLS self-signed generation, session-token lifecycle, control-frame caps, and the Origin decision — each with the source location that implements it. → *Artifact:* `test -f docs/SECURITY-REVIEW.md && grep -q 'argon2' docs/SECURITY-REVIEW.md` · *Concern:* security
- **`M2.P1.S2.T2`** — Dependency audit clean (or documented, justified allowances). → *Artifact:* `cargo audit` · *Concern:* security

## M2.P2 — Latency instrumentation

### M2.P2.S1 — Pointer-to-photon budget

- **`M2.P2.S1.T1`** — Add opt-in structured latency instrumentation stamping the capture→encode→send stages so a per-frame server-side latency figure can be logged. → *Artifact:* `cargo test --workspace --locked` · *Concern:* latency
- **`M2.P2.S1.T2`** — Write `docs/LATENCY.md` recording the measured end-to-end budget, the method, and the target ceiling from AX-1. → *Artifact:* `test -f docs/LATENCY.md` · *Concern:* docs

## M2.P3 — Accessibility audit

### M2.P3.S1 — axe-core over client routes

- **`M2.P3.S1.T1`** — Add an axe-core audit runnable over the web client routes (`/`, settings, access-code) via an npm script. → *Artifact:* `grep -q 'axe' package.json` · *Concern:* a11y
- **`M2.P3.S1.T2`** — Fix violations to WCAG 2.1 AA and verify keyboard-only operation of the settings panel. → *Artifact:* `npm run a11y` · *Concern:* a11y

## M2.P9 — Milestone quality gates

- **M2G1** — Origin enforcement is proven by test. → *Check:* `cargo test -p rylus-server origin`
- **M2G2** — Security review document exists. → *Check:* `test -f docs/SECURITY-REVIEW.md`
- **M2G3** — Latency budget is recorded. → *Check:* `test -f docs/LATENCY.md`
- **M2G4** — Dependency audit passes. → *Check:* `cargo audit`
- **M2G5** — a11y audit runs and passes. → *Check:* `npm run a11y`

---

# M3 — Notarized universal macOS DMG

Signing/notarization requires the Apple Developer prerequisites in `TODOS.md`
(`[P][$]`). The icon, bundle metadata, entitlements, and universal build are
buildable now; the signing CI steps land once the cert is provisioned (AX-8).

## M3.P1 — Bundle assets & metadata

### M3.P1.S1 — Icon & Info.plist

- **`M3.P1.S1.T1`** — Generate `packaging/macos/icon.icns` (16→1024) from the single icon source `packaging/icons/rylus.svg`. → *Artifact:* `test -f packaging/macos/icon.icns` · *Concern:* packaging
- **`M3.P1.S1.T2`** — Enrich `[package.metadata.bundle]` in `crates/rylus-server/Cargo.toml` (category, copyright, descriptions, icon, minimum-system-version) and add `NSScreenCaptureUsageDescription` + `NSAppleEventsUsageDescription`. → *Artifact:* `grep -q 'NSScreenCaptureUsageDescription' crates/rylus-server/Cargo.toml` · *Concern:* packaging

## M3.P2 — Universal build & hardened runtime

### M3.P2.S1 — lipo & entitlements

- **`M3.P2.S1.T1`** — Add `packaging/macos/entitlements.plist` with the minimal hardened-runtime entries required for signing. → *Artifact:* `test -f packaging/macos/entitlements.plist` · *Concern:* packaging
- **`M3.P2.S1.T2`** — Switch the macOS CI job to build both `x86_64-apple-darwin` and `aarch64-apple-darwin` and `lipo`-merge into a universal binary. → *Artifact:* `grep -q 'aarch64-apple-darwin' .github/workflows/build.yml` · *Concern:* ci

## M3.P3 — Sign, notarize, DMG (prereq-gated)

### M3.P3.S1 — Release pipeline

- **`M3.P3.S1.T1`** — Add codesign (`--options=runtime --timestamp --entitlements`), `notarytool submit --wait`, and `stapler staple` steps gated on the Apple secrets, producing `Rylus-<ver>-universal.dmg` via `create-dmg`. → *Artifact:* `grep -q 'notarytool' .github/workflows/build.yml` · *Concern:* packaging
- **`M3.P3.S1.T2`** — Update the macOS install section of `Readme.md` (download DMG, drag to Applications, grant Screen Recording + Accessibility). → *Artifact:* `grep -qi 'dmg' Readme.md` · *Concern:* docs

## M3.P9 — Milestone quality gates

- **M3G1** — macOS icon asset present. → *Check:* `test -f packaging/macos/icon.icns`
- **M3G2** — Entitlements present. → *Check:* `test -f packaging/macos/entitlements.plist`
- **M3G3** — CI builds a universal (arm64 + x86_64) binary. → *Check:* `grep -q 'aarch64-apple-darwin' .github/workflows/build.yml`
- **M3G4** — Notarization step wired. → *Check:* `grep -q 'notarytool' .github/workflows/build.yml`

---

# M4 — Signed Windows MSI

Signing requires the Authenticode cert and WiX prerequisites in `TODOS.md`
(`[P][$]`). The icon, version resource, DLL bundling, and WiX definition are
buildable now; `signtool` lands with the cert (AX-8). Note: today's CI ships a
bare `.exe` with no bundled FFmpeg DLLs — clean-machine installs fail, so DLL
bundling is load-bearing, not cosmetic.

## M4.P1 — Icon & version resource

### M4.P1.S1 — Embedded resources

- **`M4.P1.S1.T1`** — Generate `packaging/windows/rylus.ico` (16/32/48/64/128/256) from `packaging/icons/rylus.svg`. → *Artifact:* `test -f packaging/windows/rylus.ico` · *Concern:* packaging
- **`M4.P1.S1.T2`** — Add a `cfg(windows)`-only build script using `winresource` to embed the icon and a version block; add `winresource` under `[target.'cfg(windows)'.build-dependencies]`. → *Artifact:* `grep -q 'winresource' crates/rylus-server/Cargo.toml` · *Concern:* packaging

## M4.P2 — WiX installer & DLL bundling

### M4.P2.S1 — rylus.wxs

- **`M4.P2.S1.T1`** — Author `packaging/windows/rylus.wxs` (WiX v4): UpgradeCode, install to `%ProgramFiles%\Rylus\`, bundle `rylus.exe` + FFmpeg DLLs, Start-Menu shortcut, uninstaller, firewall exception for the server port. → *Artifact:* `test -f packaging/windows/rylus.wxs && grep -qi 'UpgradeCode' packaging/windows/rylus.wxs` · *Concern:* packaging
- **`M4.P2.S1.T2`** — Update the Windows CI job to stage FFmpeg DLLs beside `rylus.exe` and run `wix build -arch x64`. → *Artifact:* `grep -q 'wix build' .github/workflows/build.yml` · *Concern:* ci

## M4.P3 — Sign & smoke-test (prereq-gated)

### M4.P3.S1 — Release pipeline

- **`M4.P3.S1.T1`** — Add a `signtool sign /fd SHA256 /tr <timestamp> /td SHA256` step gated on the Windows cert secrets, publishing the signed `.msi`. → *Artifact:* `grep -q 'signtool' .github/workflows/build.yml` · *Concern:* packaging
- **`M4.P3.S1.T2`** — Add a silent-install smoke test (`msiexec /i … /qn` → `rylus --version` → uninstall) to catch missing DLLs and manifest errors. → *Artifact:* `grep -q 'msiexec' .github/workflows/build.yml` · *Concern:* testing
- **`M4.P3.S1.T3`** — Update the Windows install section of `Readme.md` (download `.msi`, run installer, Start-Menu shortcut). → *Artifact:* `grep -qi 'msi' Readme.md` · *Concern:* docs

## M4.P9 — Milestone quality gates

- **M4G1** — Windows icon present. → *Check:* `test -f packaging/windows/rylus.ico`
- **M4G2** — WiX definition present with an UpgradeCode. → *Check:* `grep -qi 'UpgradeCode' packaging/windows/rylus.wxs`
- **M4G3** — CI bundles DLLs and builds the MSI. → *Check:* `grep -q 'wix build' .github/workflows/build.yml`
- **M4G4** — Silent-install smoke test wired. → *Check:* `grep -q 'msiexec' .github/workflows/build.yml`

---

# M5 — 1.0.0 release

Resolve versioning, verify the quickstart on every OS, and cut the tag (AX-8,
and the Done-at-1.0.0 checklist in `VISION.md`).

## M5.P1 — Versioning decision

### M5.P1.S1 — Tag history

- **`M5.P1.S1.T1`** — Record an ADR resolving whether to back-annotate the 0.7–0.17 line or start the tag history clean at `v1.0.0` (Cargo is at 0.17.0; git is tagged only through v0.6.1). → *Artifact:* `ls docs/adr/ADR-*version* 2>/dev/null | head -1` · *Concern:* release
- **`M5.P1.S1.T2`** — Set the workspace version to `1.0.0` in `Cargo.toml` and refresh `Cargo.lock`. → *Artifact:* `grep -q '^version = "1.0.0"' Cargo.toml` · *Concern:* release

## M5.P2 — Release verification

### M5.P2.S1 — Quickstart & changelog

- **`M5.P2.S1.T1`** — Verify the `Readme.md` quickstart end to end on Linux, macOS, and Windows from the signed artifacts, and record the result. → *Artifact:* `cargo run -q -p rylus-server -- --self-test` · *Concern:* release
- **`M5.P2.S1.T2`** — Add the `[1.0.0]` section to `CHANGELOG.md` moving items out of `[Unreleased]`. → *Artifact:* `grep -q '\[1.0.0\]' CHANGELOG.md` · *Concern:* docs

## M5.P9 — Milestone quality gates

- **M5G1** — Version is 1.0.0. → *Check:* `grep -q '^version = "1.0.0"' Cargo.toml`
- **M5G2** — CHANGELOG has a 1.0.0 entry. → *Check:* `grep -q '\[1.0.0\]' CHANGELOG.md`
- **M5G3** — Full workspace + frontend tests pass. → *Check:* `cargo test --workspace --locked`
- **M5G4** — Self-test green. → *Check:* `cargo run -q -p rylus-server -- --self-test`
</content>
