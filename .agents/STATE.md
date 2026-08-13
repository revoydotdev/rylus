# Swarm State Ledger

> Append-only-ish ledger the supervisor maintains each tick. Keep newest at top.
> Status keys: CLAIMED · IN_PROGRESS · DONE · BLOCKED · GATE-FAILED

<!-- CONTROL: machine-read; supervisor updates these two lines -->
- MILESTONE_PHASE: NORMAL
- CURRENT_MILESTONE: M3

## 2026-08-13 tick — NORMAL: M3 gate decomposition + 3 dispatched concerns

First NORMAL tick of M3 (`ledger.py status` showed 0 done, `next --milestone
M3` showed 6 unclaimed). Decomposed M3G1-M3G4 into explicit gate-closing
todos under a new `M3.P9.S1`/`M3.P9.S2` section in ROADMAP.md, mirroring the
M1.P9/M2.P9 pattern: M3.P9.S1.T1-2 (icon asset, entitlements) and
M3.P9.S2.T1-2 (universal CI build, notarization wiring). `ledger.py next
--milestone M3` now shows 10 unclaimed todos (was 6).

Self-heal pass on all 10 unclaimed M3 todos: all 10 NEEDS-WORK (genuinely
unbuilt, greenfield packaging — none recorded for free).

Noted but not yet acted on: `M3.P3.S1.T2`'s artifact command in ROADMAP.md
(`grep -qi 'dmg' Readme.md`) references `Readme.md` but the real file is
`README.md` — wrong case, will never match on this case-sensitive filesystem.
Left the roadmap prose untouched (out of scope for concerns not claimed this
tick) but flagging here so a future tick fixes the command instead of
re-discovering the false NEEDS-WORK.

CLAIMED this tick (3 disjoint concerns, dispatched to sonnet workers):
- CLAIMED `M3.P1.S1.T1` + `M3.P9.S1.T1`/M3G1 — concern `macos-icon-icns` —
  generate `packaging/macos/icon.icns` from `packaging/icons/rylus.svg`
  (touches `packaging/macos/icon.icns`, `scripts/gen-icons.sh`)
- CLAIMED `M3.P1.S1.T2` — concern `macos-bundle-metadata` — enrich
  `[package.metadata.bundle]` in `crates/rylus-server/Cargo.toml` (touches
  only that file)
- CLAIMED `M3.P2.S1.T1` + `M3.P9.S1.T2`/M3G2 — concern `macos-entitlements`
  — add `packaging/macos/entitlements.plist` (touches only that new file)

Deliberately left unclaimed this tick: `M3.P2.S1.T2` + `M3.P3.S1.T1`
(both touch `.github/workflows/build.yml` — reserved for a future tick to
avoid intra-tick file contention with each other), `M3.P3.S1.T1` proper is
also prereq-gated per the roadmap's own note (Apple Developer cert not yet
provisioned), and `M3.P3.S1.T2` (docs) has the Readme.md casing bug above.

All 3 workers PASSED, independently re-verified (diff read, checks re-run
myself on the integrated tree, not trusted from self-report) before
recording done:

- DONE — `M3.P1.S1.T1` + `M3.P9.S1.T1`/M3G1 (concern: `macos-icon-icns`) —
  integrated at `ba5e949`. Root cause: `scripts/gen-icons.sh`'s icns block
  was gated on `iconutil` (macOS-only, absent on this Linux box), so it
  silently no-op'd with zero error. Fix: added a Linux-capable fallback —
  `rsvg-convert` renders each size directly from `packaging/icons/rylus.svg`
  (not upscaled from existing PNGs), an inline python3 assembler builds the
  ICNS container per the documented OSType table (cross-checked against the
  rust-icns crate + the Apple Icon Image format reference before writing
  bytes, not from memory). Re-verified myself: `test -f
  packaging/macos/icon.icns` exit 0, magic bytes `icns` at offset 0, all 10
  OSType entries parse as well-formed length-framed PNG chunks (independent
  python3 parse, not just trusting the worker's `file` output).
- DONE — `M3.P2.S1.T1` + `M3.P9.S1.T2`/M3G2 (concern: `macos-entitlements`)
  — integrated at `6ffa467`. Investigated whether
  `com.apple.security.cs.disable-library-validation` is genuinely needed
  (TODOS.md says avoid unless required): traced `ffmpeg-sys-next`'s build.rs
  — no `static`/`build` feature enabled, so it links dynamically via
  pkg-config against the Homebrew-provisioned FFmpeg on the self-hosted
  macOS runner (`build.yml` never runs `brew install ffmpeg` itself, i.e.
  those dylibs are pre-existing and not signed with our Team ID) — hardened
  runtime Library Validation would refuse to load them, so the exception is
  real, not precautionary. `allow-jit` genuinely omitted (grepped
  `Cargo.lock` for JIT-capable crates, none found; pure AOT Rust per AX-5).
  Re-verified myself: `test -f packaging/macos/entitlements.plist` exit 0,
  well-formed XML (independent `xml.dom.minidom` parse).
- DONE — `M3.P1.S1.T2` (concern: `macos-bundle-metadata`) — integrated at
  `a35e240`. Enriched `[package.metadata.bundle]` (category, copyright,
  descriptions, icon path, `osx_minimum_system_version`). Verified the
  actual `cargo-bundle` manifest schema against its real docs (not vendored
  in this repo, not installed in this sandbox) before writing keys — the
  real, documented mechanism for injecting arbitrary Info.plist keys is
  `osx_info_plist_exts` (a list of file paths whose bare `<key>/<value>`
  contents get appended into the generated Info.plist), wired to a new
  `packaging/macos/info-plist-extras.plist` carrying the real
  `NSScreenCaptureUsageDescription`/`NSAppleEventsUsageDescription` strings
  — not a decorative comment standing in for missing wiring. Re-verified
  myself: `grep -q NSScreenCaptureUsageDescription
  crates/rylus-server/Cargo.toml` exit 0, `cargo metadata --no-deps
  --format-version=1 -q` exit 0 (Cargo.toml still parses cleanly with the
  new keys).

2 learnings logged (`macOS icns`/iconutil-Linux-silent-noop trap,
`cargo-bundle osx_info_plist_exts` real mechanism) — both non-obvious and
likely to recur for the M4 Windows MSI work.

No a2a-requests were needed this tick (0 pending at both the pre-dispatch
and post-dispatch polls) — all 3 workers resolved their own research
(iconutil unavailability, cargo-bundle's real schema, FFmpeg linking mode)
by reading the authoritative source rather than escalating.

`ledger.py next --milestone M3` now shows 5 unclaimed (`M3.P2.S1.T2`,
`M3.P3.S1.T1`, `M3.P3.S1.T2`, `M3.P9.S2.T1`, `M3.P9.S2.T2`) — M3G3/M3G4 not
yet closed, so M3 is NOT candidate-complete; CONTROL stays `NORMAL`/`M3`.
All 3 worktrees destroyed, `concern/*` branches deleted post-merge.

## 2026-07-22 tick — M2 AUDIT PASS → master promoted, advance to M3 NORMAL

First CLEAN tree since the DIRTY blocker cleared (operator A2A-bus WIP is now
committed as `61af215`). Ran the sole-turn M2 audit — no workers. All five
gates re-verified with fresh runs THIS tick:

- **M2G1** origin test — `cargo test -p rylus-server origin`: 3 passed
  (`ws_upgrade_foreign_origin_rejected`, `_absent_origin_accepted`,
  `_same_origin_accepted`), exit 0.
- **M2G2** `docs/SECURITY-REVIEW.md` exists + `argon2` present.
- **M2G3** `docs/LATENCY.md` exists — substantive (server capture→encode→send
  p95 = 792 µs, honestly scoped vs the AX-1 ~7 ms ceiling, method + repro).
- **M2G4** `cargo audit`: exit 0; two `quick-xml` advisories narrowly
  allow-listed in `.cargo/audit.toml` with justified GUI-only-path rationale
  (M2.P1.S2.T2).
- **M2G5** `npm run a11y`: 0 axe violations across `/`, `/settings.html`,
  `/access_code.html`; keyboard-only settings-panel check all PASS.
- `ledger.py check --rerun`: PASS (28 done todos, structural+rerun).

Axioms AX-1 (latency measured, not asserted) and AX-6 (secure-by-default,
Origin proven wired by test) satisfied. Verified the roadmap prose
"foreign/absent Origin is rejected" is loose but NOT a drift: the
implemented+tested+documented behavior (foreign rejected, absent accepted for
non-browser clients) is a deliberate, source-cited CSWSH-defense decision in
SECURITY-REVIEW.md §1 — coherent, not an oversight.

**Promotion:** master is aswarm-owned (tip was the M1 promotion commit), not
checked out in any worktree, in sync with origin, and a clean ancestor of
integration. Fast-forwarded master `3196d05..61af215` and pushed; also pushed
integration `939143a..61af215`. Both branches 0/0 vs origin. Advanced CONTROL
to `M3 NORMAL`.

## 2026-07-21 tick — SELF-PARK (4h): same DIRTY blocker, no new info

Third consecutive tick blocked at preflight by `DIRTY:2` — the identical
uncommitted operator WIP on harness files (`.agents/dashboard/governance.py`
+105/-4 A2A-bus API, `.agents/daedalus/TICK_PROMPT.md` +9/-5). No new
information since the immediately-preceding (17:24) tick entry: recovery
checks still clean (fresh lock, no `concern/*` worktrees/branches,
`integration` 0/0 vs origin & 28 ahead of `origin/master`, `.revoy/*`
worktrees are `/revoy`'s), governance not paused, 0 unread, block already
posted twice (13:14, 17:24) so NOT re-posted. Zero workers, zero ledger
progress, blocker is operator-only (commit/stash the A2A-bus WIP). Wrote
`.agents/SELF_PARK` (4h) so the dispatch wrapper parks the project instead
of re-burning ticks on an unchanged blocker. M2 AUDIT still pending; runs
the first tick that sees a CLEAN tree.

## 2026-07-21 tick — STOPPED at preflight: DIRTY tree, no audit run

Preflight classified `DIRTY:2 files` — foreign uncommitted edits to the
swarm harness itself: `.agents/daedalus/TICK_PROMPT.md` (+9/-5) and
`.agents/dashboard/governance.py` (+105/-4). These are operator/infra
work-in-progress, not project residue, so per STEP 2 the tick did NOT
clobber, commit, or revert them and did NOT proceed to the pending M2 AUDIT.

Recovery checks (safe, non-clobbering) all clean: no stale lock, no
`concern/*` worktrees or branches, `integration` in sync with
`origin/integration` (0/0) and 27 ahead of `origin/master` (expected). The
three `.revoy/*` worktrees (`feat/pwa-a11y-wcag`, `ryl/a11y-audit`,
`ryl/docs-1.0`) belong to the `/revoy` tool, not aswarm — left untouched.
Governance: not paused, no directives, no unread. M2 AUDIT remains pending
for the next tick once the tree is clean. Posted the block to governance.



Self-heal pass on the 9 unclaimed M2 todos found 2 legitimate (real,
non-vacuous evidence): `M2.P9.S1.T1` (3 real origin tests pass, not the
prior 0-filtered false positive) and `M2.P9.S1.T2` (security doc exists,
cites argon2). Deliberately did NOT self-heal `M2.P2.S1.T1`/`M2.P9.S2.T1`
(no latency instrumentation existed yet, only comments) or
`M2.P3.S1.T2`/`M2.P9.S2.T2` — found `npm run a11y` **always exited 0
regardless of violations found**, the same vacuous-gate bug class as the
earlier `cargo test` name-filter false positive; fixing that gate was
folded into the a11y concern's scope rather than papered over.

Claimed and dispatched 3 disjoint sonnet workers, all independently
re-verified (diff read, checks re-run myself, not trusted from self-report)
before integrating:

- DONE — `M2.P1.S2.T2` + `M2.P9.S1.T3`/M2G4 (concern: `cargo-audit-clean`) —
  integrated `concern/cargo-audit-clean` at `dbfced6`. `cargo audit` was
  genuinely failing on 7 real vulnerabilities (quick-xml x4 via the AT-SPI/
  Wayland build chain, rustls-webpki x3 via `rustls`) — the 7 previously-logged
  "warnings" (unmaintained/unsound/yanked) don't affect the gate's exit code
  by default and were correctly left alone. Fix: `rustls-webpki` bumped
  0.103.10→0.103.13 (in-range lockfile update). quick-xml has no
  non-disruptive fix (would require cascading eframe/egui/winit/accesskit
  major bumps) — added a narrow, cited `.cargo/audit.toml` ignore for just
  those 2 advisories, verified for real by toggling the file and confirming
  exit 1→0. **Found and fixed one accuracy issue before integrating:** the
  worker's rationale cited "ADR-0001" for the LAN-only threat model; the real
  decision is ADR-0003 (ADR-0001 is the generic meta-ADR) — same mismatch a
  much earlier audit tick flagged. Fixed with a follow-up commit before
  merge.
- DONE — `M2.P3.S1.T2` + `M2.P9.S2.T2`/M2G5 (concern: `a11y-violations-fixed`)
  — integrated `concern/a11y-violations-fixed` at `a5d1326`. All 7 axe-core
  violation groups fixed (viewport zoom re-enabled, sr-only `<h1>`, landmark
  wrapping on `/` and `/settings.html`, color-contrast fixes on `.info` and
  the access-code screen). Independently recomputed WCAG contrast ratios by
  hand (relative-luminance formula) rather than trusting the claimed
  numbers — they matched exactly once the real ancestor background was
  traced (`.auth-card` → `--background-color-1`, not the outer page bg).
  The vacuous `a11y.mjs` exit-code gate is now real — proved it myself by
  reintroducing the old failing color and confirming exit 1, then reverting.
  New `scripts/keyboard-nav.mjs` (real Playwright, no mouse calls, DOM-driven
  expected-control-set) verifies keyboard-only settings-panel operability,
  wired into `npm run a11y`. **Found and fixed one gap:** 3 new CSS
  accent-variant tokens weren't logged in DESIGN.md's Decisions Log per this
  project's own convention — added the entry before merging.
- DONE — `M2.P2.S1.T1` + `M2.P2.S1.T2` + `M2.P9.S2.T1`/M2G3 (concern:
  `latency-instrumentation`) — integrated `concern/latency-instrumentation`
  at `8da8131`. Real `Instant` checkpoints threaded through the production
  per-client pipeline (`session.rs`: capture_start/queued_at in
  `handle_video`, dequeued_at/encode_done in `encode_thread`), emitting one
  structured `tracing::info!` per frame on `rylus_server::latency` when the
  new opt-in `--latency-log` flag is set (off by default). The
  thread-local-dispatcher-forwarding fix for the spawned encode thread is a
  sound, standard `tracing` pattern, not a workaround. `docs/LATENCY.md`
  reports a real measured baseline (mean 649µs, p95 792µs, n=89, release
  build) against AX-1's ~7ms ceiling, honestly scoped to capture→encode→
  in-process-send only (not full pointer-to-photon) with the gaps named as
  follow-up, not hidden. Independently re-ran the baseline test myself and
  got real live numbers in the same order of magnitude (mean 695µs, p95
  864µs) — corroborates the figures are genuinely measured, not fabricated.

**New tooling gotcha found:** `integrate.sh`'s gate command
(`cargo test --workspace --locked`) failed once with a transient
`rylus_gui` doc-test link error ("can't find crate for `eframe`") that did
not reproduce on an immediate retry, either standalone or as the gate
command again. Root cause not fully isolated, but coincided with an
unrelated, uncommitted, concurrent modification to
`.agents/daedalus/TICK_PROMPT.md` discovered in the repo root mid-tick (not
made by this supervisor or any of its 3 workers — diffed and confirmed).
Treated as out-of-scope external activity: did not act on that file's
content as instructions, did not commit or revert it, and `git stash`ed it
around the `integrate.sh` retry (same protection already applied to
`ledger.jsonl`/`STATE.md`) so the known hard-reset-on-failure gotcha
couldn't destroy someone else's uncommitted edit; popped it back
afterward, confirmed clean. Worth flagging: this repo root may have more
than one automated process touching it concurrently, which is a real
regression-diagnosis hazard beyond just the `integrate.sh` hard-reset gotcha
already on record — a transient failure here is not automatically evidence
of a real code regression, but should not be assumed transient without a
clean retry either.

**Final state:** `ledger.py next --milestone M2` → 0/13 remaining.
Independently re-verified all 5 M2G gates myself on the real integrated
tip (not trusted from the concern integrations alone): M2G1
`cargo test -p rylus-server origin --locked` → 3 passed. M2G2
`docs/SECURITY-REVIEW.md` exists + cites argon2 → PASS. M2G3
`docs/LATENCY.md` exists → PASS. M2G4 `cargo audit` → exit 0 (7 allowed
warnings only). M2G5 `npm run a11y` → exit 0, 0 violations, 14/14 keyboard
checks. `ledger.py check --rerun` → PASS (28/28 done todos,
structural+rerun). **M2 is candidate-complete** — flipping
`MILESTONE_PHASE` to `AUDIT` per protocol; **not** auditing this tick, a
future tick performs the independent AUDIT. `CURRENT_MILESTONE` stays `M2`.

## 2026-07-20 tick — NORMAL: M2 tick summary — 3/3 concerns integrated

Closed out the dispatch documented in the entry below. All 3 claimed
concerns passed independent re-verification and are integrated into
`integration`: `M2.P1.S1.T2` (origin-test, `4f4d696`), `M2.P1.S2.T1`
(security-review-doc, `f45d545`), `M2.P3.S1.T1` (axe-a11y-wiring,
`14d5ff8`). `ledger.py check --rerun` → PASS (19/19 done todos,
structural+rerun). `worktree-check.sh` → clean, no orphaned worktrees, no
`concern/*` branches left. All 3 worktrees destroyed
(`git worktree remove` + `git branch -d`, post-merge — see the tooling
gotcha note below for why not `scripts/worktree.sh destroy`).

`ledger.py next --milestone M2` → 9 of 13 unclaimed
(`M2.P1.S2.T2` cargo-audit-clean, `M2.P2.S1.T1`/`T2` latency instrumentation
+ doc, `M2.P3.S1.T2` a11y-violations-fixed, and the 5 `M2.P9.*` gate-closing
todos). `MILESTONE_PHASE` stays `NORMAL` — M2 is not candidate-complete.

## 2026-07-20 tick — NORMAL: M2 dispatch (self-heal correction + 3 concerns)

Self-heal pass before dispatch surfaced a real gap: `M2.P1.S1.T1` (origin
audit/grep artifact) self-healed legitimately — read `web.rs:279-301,535-546`
and confirmed `ws_origin_matches_host` is genuinely wired into the `/ws`
upgrade path (403 on mismatch). But `M2.P1.S1.T2`'s self-heal via
`cargo test -p rylus-server origin` was VACUOUS — `cargo test` exits 0 even
when the name filter matches 0 tests (67 filtered out, 0 run) — so it was
killed via `ledger.py kill` with reason logged, matching the same class of
false-positive the M1 audit caught for `protocol_version`.

Also probed `cargo audit` directly (informational, not claimed this tick): it
currently fails with 7 advisories (anyhow downcast_mut unsoundness, memmap2
unchecked offset, rand 0.8/0.9 logger unsoundness, a yanked `spin` build) —
real work for `M2.P1.S2.T2`/`M2.P9.S1.T3` in a future tick, not a self-heal.

CLAIMED this tick (3 disjoint concerns), all dispatched to sonnet workers in
worktrees off `integration`:
- DONE — `M2.P1.S1.T2` (concern: `origin-test`) — integrated `concern/origin-test`
  at `eb7bec4` (ff-merge `4f4d696`) — real `#[test]`s boot a loopback `Rylus`
  server and drive raw-TCP `/ws` upgrades: same-origin → 101, foreign Origin →
  403, absent Origin → 101. Verified `cargo test -p rylus-server origin` exit 0
  independently (3 passed) both pre- and post-integration, not just trusted
  the worker's self-report.
- DONE — `M2.P1.S2.T1` (concern: `security-review-doc`) — integrated
  `concern/security-review-doc` at `1ab1142` (ff-merge `f45d545`) — wrote
  `docs/SECURITY-REVIEW.md` (6 sections: Origin, argon2, rate-limiting, TLS,
  session tokens, control-frame caps), each cited to real source lines.
  Spot-checked ~10 citations against the actual code myself before
  integrating (argon2 0.5.3/rcgen 0.13.2 versions, `MAX_FAILED_ATTEMPTS=5`/
  `RATE_LIMIT_WINDOW=60s`/`LOCKOUT_DURATION=30s`, session cookie
  `HttpOnly`+`SameSite=Strict` w/ no `Secure`, TLS `Auto` mode's `/tmp/rylus`
  unhardened-permissions gap, binary-frame-size-uncapped gap) — all matched.
  Doc honestly documents 5 weak points rather than only listing strengths;
  none of those are fixed this tick (out of scope), but worth future todos.
- DONE — `M2.P3.S1.T1` (concern: `axe-a11y-wiring`) — integrated
  `concern/axe-a11y-wiring` at `a60068b` (ff-merge `14d5ff8`) — real
  `scripts/a11y.mjs` builds the client bundle, serves the three static
  routes (`/`, `/settings.html`, `/access_code.html`) from a throwaway local
  HTTP server, drives headless Chromium via Playwright, injects real
  `axe-core`, and runs `axe.run()`. Independently reran `npm install` +
  `npm run a11y` on the integrated tree myself (not just trusted the
  worker): genuinely executes, finds 7 real violation groups across the 3
  routes (meta-viewport, missing h1/main landmarks, color-contrast,
  unlandmarked regions) — fixing those is the separate `M2.P3.S1.T2` todo,
  out of scope this tick.

**Tooling gotcha found:** `scripts/integrate.sh`'s failure path does
`git reset --hard "$PRE_REBASE_SHA"` on whatever branch is currently checked
out when the rebase step fails — if the supervisor's repo root had
uncommitted changes to shared files (`.agents/STATE.md`/`ledger.jsonl`) at
invocation time, "You have unstaged changes" aborts the rebase and the hard
reset silently discards those uncommitted edits, not just the failed
rebase's partial state. Hit this once this tick (lost and had to re-record
the `M2.P1.S1.T1`/`M2.P1.S1.T2` self-heal entries above). Workaround used for
the rest of this tick: `git stash` any shared-file edits before calling
`integrate.sh`, `git stash pop` after. Worth a future todo to make
`integrate.sh` refuse to run (not silently eat working-tree state) when the
repo root has uncommitted changes before it ever touches HEAD.

## 2026-07-20 tick — NORMAL: M2 gate decomposition

First NORMAL tick of M2 (`ledger.py status --milestone M2` showed 0 done).
Decomposed M2G1-M2G5 into explicit gate-closing todos under a new
`M2.P9.S1`/`M2.P9.S2` section in ROADMAP.md, mirroring the M1.P9 pattern:
M2.P9.S1.T1-3 (Origin test, security doc, cargo audit) and M2.P9.S2.T1-2
(latency doc, a11y audit). `ledger.py next --milestone M2` now shows 13
unclaimed todos (was 8). Supervisor dispatch for feature concerns follows
in this same tick.

## 2026-07-20 tick — AUDIT: PASS, master fast-forwarded to M1

Sole-turn audit, no worker agents dispatched. Independently re-verified all
of M1 against its 5 gates, VISION.md axioms, and the ADRs, on `integration`
tip `3196d05` — not a rubber stamp of the prior REMEDIATION tick's
self-report, and not re-trusting anything the prior AUDIT:FAIL tick already
checked without re-running it myself.

**Read for context (not re-litigated, just grounding):** `VISION.md` (all 8
axioms), `docs/adr/ADR-0001..0004`, and the top ~160 lines of this file
covering the prior AUDIT:FAIL (vacuous `protocol_version_is_three` test) and
its REMEDIATION (replaced with `protocol_version_matches_docs`).

**Re-ran every gate for real:**
- `python3 scripts/ledger.py check --rerun` → PASS, 15/15 M1 todos
  structural+rerun clean.
- M1G1 `cargo test --workspace --locked` → 0 failed across every crate
  (rylus-core 89+, rylus-transport 14, rylus-server suites, doc-tests all
  0/0 as expected).
- M1G2 `cargo clippy --all-targets -- -D warnings` → exit 0, no warnings.
- M1G3 `cargo fmt -- --check` → exit 0, no diff.
- M1G4 `cargo run -q -p rylus-server -- --self-test` → exit 0. Read
  `crates/rylus-server/src/self_test.rs` end to end: real testsrc capture,
  real libx264 GOP encode (asserts packets emitted), real loopback bind +
  hand-rolled HTTP Upgrade request over a raw TcpStream asserting `101` in
  the response status line. `main.rs:66-68` wires `conf.self_test` to
  `self_test::run()` + `process::exit`, not a dead flag. TLS is disabled
  only inside the self-test's own `internal_conf` (`self_test.rs:156`), not
  the default CLI posture — no ADR-0004 drift.
- M1G5 `test -f docs/PROTOCOL.md && grep -q HeartbeatAck ...` → PASS. Read
  `docs/PROTOCOL.md` in full (712 lines): genuinely documents the handshake,
  Origin check, text/binary framing split, every `MessageInbound`/
  `MessageOutbound` variant (including the `batched_pointer_events` serde
  rename, `HelloNack` version-guard, `ClientRtt`, `RequestKeyframe`,
  `BufferHealth`), heartbeat v3 RTT/jitter classification, and states
  `PROTOCOL_VERSION = 3` / `MIN_CLIENT_PROTOCOL_VERSION = 2` matching
  `protocol.rs:7,14` exactly. Explicitly ties itself to AX-1/AX-3/ADR-0003;
  no alternate-transport language anywhere.

**Independently proved the protocol-doc-drift guard fires — did not trust
the prior tick's claim.** Scratch-edited `crates/rylus-core/src/protocol.rs`
line 7 to `PROTOCOL_VERSION: u32 = 4` (uncommitted), ran
`cargo test -p rylus-core protocol_version --locked`: FAILED with
`docs/PROTOCOL.md documents PROTOCOL_VERSION = 3, but protocol.rs defines
PROTOCOL_VERSION = 4 -- update the doc`. Then `git checkout --
crates/rylus-core/src/protocol.rs` to revert, re-ran the same test: 1
passed. `git status` confirmed clean before proceeding. The guard is real,
not vacuous.

**Bench harness — ran the actual regression gate, not just read it.**
`bash scripts/bench-gate.sh`: baseline 667.94µs (`BASELINE.md`, real
recorded run with host/FFmpeg version), measured 678.15µs this run, delta
+1.53%, threshold 15% → PASS. `crates/rylus-encode/benches/encode.rs` drives
real `VideoEncoder::encode()` over a fixed synthetic BGR0 frame through the
real libx264 software path — not a stub. `.github/workflows/build.yml`'s
`quality` job runs `Self-test` and `Encode benchmark regression gate` as
real steps, and every other job (`build-linux`, `build-linux-alpine`,
`build-macos`, `build-windows`) declares `needs: quality` — coherent, not
orphaned.

**No VISION/ADR drift found.** WebSocket-only/LAN-only (AX-3/ADR-0003),
secure-by-default (AX-6/ADR-0004), pure-Rust workspace boundary
(AX-5/ADR-0002) all held across everything read this tick.

**Verdict: PASS.** All 5 gates hold for real, ledger re-verifies clean,
every artifact spot-checked is substantive (not vacuous/stubbed), no
drift.

**Branch actions taken:** `master` (`bcacb48`) and `integration`
(`3196d05`) were both confirmed in sync with their respective remotes
first. `git merge-base --is-ancestor master integration` → true (42 commits
ahead, clean ff-only path). Ran `git checkout master && git merge --ff-only
integration` → fast-forwarded cleanly (49 files, no conflicts, no rewrite).
Pushed with `git push origin master` (normal push, not force) →
`bcacb48..3196d05 master -> master` succeeded. Switched back to
`integration` afterward. Did not touch `origin/integration`.

`MILESTONE_PHASE` set to `NORMAL`, `CURRENT_MILESTONE` advanced to `M2`
(Security review & latency verification) per `ROADMAP.md:82` — the next
milestone after M1 in the file, not a guess.

## 2026-07-20 tick — REMEDIATION: fixed vacuous protocol-doc-drift test (M1.P2.S1.T3)

Narrow fix for the prior tick's sole AUDIT:FAIL finding. Replaced
`protocol_version_is_three` (`crates/rylus-core/src/protocol.rs`) — which
only compared `PROTOCOL_VERSION` to a second hardcoded literal `3` in the
same file and never touched `docs/PROTOCOL.md` — with
`protocol_version_matches_docs`, which reads `docs/PROTOCOL.md` at compile
time via `include_str!`, parses the `pub const PROTOCOL_VERSION` /
`pub const MIN_CLIENT_PROTOCOL_VERSION` declarations the doc quotes
verbatim from this file, and asserts both against the real constants.

Proved the guard actually fires before committing: scratch-bumped
`PROTOCOL_VERSION` to `4` without touching the doc, confirmed
`protocol_version_matches_docs` FAILED with a clear drift message, then
reverted the scratch edit (`git checkout -- crates/rylus-core/src/protocol.rs`)
before writing the real fix.

Verification re-run for real, not trusted: `cargo test -p rylus-core
protocol_version --locked` (1 passed), `cargo fmt -- --check` (exit 0, no
diff), `cargo clippy --all-targets -- -D warnings` (exit 0, no warnings),
then `cargo test --workspace --locked` (full workspace, 0 failed across
all crates) as a final regression check on `integration` tip. Fix commit:
`38054b0`. Ledger: `python3 scripts/ledger.py done --todo M1.P2.S1.T3
--commit 38054b0 --concern protocol-doc-drift-test --cmd 'cargo test -p
rylus-core protocol_version --locked' --run` recorded a genuine 0-exit
verification (superseding the prior self-healed, no-commit record).
`ledger.py check --rerun` → PASS (15/15 done todos).

M1's only blocking finding is now closed. `MILESTONE_PHASE` set to
`AUDIT`, `CURRENT_MILESTONE` stays `M1` — a future tick performs the
independent AUDIT; this tick did not self-certify the audit. Did not touch
`master`/`origin` beyond a normal push to `integration`.

## 2026-07-20 tick — AUDIT: FAIL (1 blocking issue, master NOT fast-forwarded)

Independent re-verification of M1 against its 5 gates plus substance, on
`integration` tip `c134d9c`. Not a rubber stamp of the prior REMEDIATION
tick's self-report.

**All 5 gates genuinely green, re-run for real, not trusted from the
ledger:**
- M1G1 `cargo test --workspace --locked`: full run PASS, all crates 0
  failed. Separately ran `cargo test -p rylus-server mdns:: --locked --
  --test-threads=8` **10x back-to-back**: 15 passed / 0 failed every time —
  the compare_exchange fix from the prior remediation tick
  (`crates/rylus-server/src/mdns.rs:128-147`) is confirmed solid, no flake
  reproduced. Read the fix: single `CACHED.compare_exchange(0, mixed,
  Relaxed, Relaxed)` — winner and every loser converge on the same `mixed`
  value via the `Err(winner) => format_suffix(winner)` arm. Correctly
  reasoned, no remaining race.
- M1G2 `cargo clippy --all-targets -- -D warnings` → exit 0, no warnings.
- M1G3 `cargo fmt -- --check` → exit 0, no diff.
- M1G4 `cargo run -q -p rylus-server -- --self-test` → exit 0. Read
  `crates/rylus-server/src/self_test.rs` in full: real `testsrc` capture,
  real `VideoEncoder`/libx264 GOP encode (asserts `emitted != 0`), real
  `Rylus::start` bind on an ephemeral loopback port, real hand-rolled HTTP
  Upgrade request over a raw `TcpStream` asserting the response status line
  contains `101`. Confirmed `main.rs:66-68` actually wires `conf.self_test`
  to call `self_test::run()` and `process::exit` on its result — not a
  disconnected flag. The M1.P1.S1.T3 in-process test
  (`self_test_run_passes_and_tears_down_cleanly`) genuinely calls `run()`
  and asserts `true`; confirmed non-vacuous (was the prior tick's fix).
- M1G5 `test -f docs/PROTOCOL.md && grep -q HeartbeatAck ...` → PASS.
  Diffed `docs/PROTOCOL.md` against every variant in
  `crates/rylus-core/src/protocol.rs` by hand: all `MessageInbound` and
  `MessageOutbound` variants are documented with accurate JSON shapes,
  including the `batched_pointer_events` serde-rename exception, the
  `HelloNack` version-guard, `ClientRtt`, `RequestKeyframe`, and the stated
  `PROTOCOL_VERSION = 3` / `MIN_CLIENT_PROTOCOL_VERSION = 2` constants
  (verified these two numbers against `protocol.rs:7,14` — exact match). No
  drift found. Consistent with AX-3/ADR-0003 (LAN-only, single-WebSocket
  transport) — no alternate transport implied anywhere in the doc.
- `python3 scripts/ledger.py check --rerun` → PASS (15/15 done todos,
  structural+rerun).

**Encode bench harness — real, not cosmetic.**
`crates/rylus-encode/benches/encode.rs` drives the actual
`VideoEncoder::encode()` over a real libx264 software path with a fixed
synthetic BGR0 frame (not a stub). `BASELINE.md` records a real measured
run (667.94µs mean, host CPU/FFmpeg version stated). Read
`scripts/bench-gate.sh` in full and ran it directly: it parses the baseline
mean out of `BASELINE.md` (not hardcoded), runs the bench for real, parses
the measured mean, computes `(measured-baseline)/baseline*100`, and exits
non-zero if that exceeds `THRESHOLD_PCT` (15, overridable via env). This
run: measured 680.52µs, delta +1.88%, PASS — genuinely would fail on a real
regression, not a rubber-stamp. `.github/workflows/build.yml`'s `quality`
job wires both `Self-test` (line 37-38) and `Encode benchmark regression
gate` (`./scripts/bench-gate.sh`, line 39-40) as real steps ahead of every
`needs: quality` build job — a non-zero exit from either genuinely fails
CI.

**One blocking finding: `M1.P2.S1.T3`'s test does not actually check
doc/code alignment — it's a hardcoded tautology, not a drift guard.**

The todo's own text: "Add a test asserting the documented protocol version
matches the constant in code, so the doc cannot silently drift." The
artifact check is `cargo test -p rylus-core protocol_version`, which
matches exactly one test: `protocol_version_is_three`
(`crates/rylus-core/src/protocol.rs:967-970`):

```rust
#[test]
fn protocol_version_is_three() {
    assert_eq!(PROTOCOL_VERSION, 3);
}
```

This asserts the `PROTOCOL_VERSION` constant equals a **hardcoded literal
`3`** — it never reads, parses, or references `docs/PROTOCOL.md` in any
way. Confirmed with `grep -rn "PROTOCOL.md\|include_str" crates/`: zero
matches anywhere in the workspace. So if a future change bumps
`PROTOCOL_VERSION` to `4` in `protocol.rs` *and* updates this test's
literal to `4` (which a developer would naturally do together, since
they're two lines apart in the same file) but forgets to update
`docs/PROTOCOL.md`, this test keeps passing while the doc silently goes
stale — exactly the failure mode the todo was written to prevent. The test
provides zero actual protection against doc/code drift; it only protects
against `PROTOCOL_VERSION` drifting from an arbitrary number restated a few
lines below it in the same file. This is a "safeguard that is documented
but not wired" per this project's own AX-6 (`VISION.md:61`: "A safeguard
that is documented but not wired is treated as a defect") and the same
class of vacuous-artifact problem the immediately prior remediation tick
already fixed once for `M1.P1.S1.T3` (self_test.rs) — this is the same bug
pattern recurring in a sibling todo that wasn't in scope for that fix.

Ledger record for `M1.P2.S1.T3` (`{"verified_by":{"cmd":"cargo test -p
rylus-core protocol_version","exit":0}}`, self-healed, no commit) reflects
a real passing test, but the test doesn't do what the todo — or the
milestone's "wire protocol is documented ... and matches
`rylus-core::protocol`" 1.0.0 criterion (`VISION.md:85-87`) — actually
requires.

**Verdict: FAIL.** Did not touch `master`/`origin`. `MILESTONE_PHASE` set
to `REMEDIATION`, `CURRENT_MILESTONE` stays `M1`. Remediation scope for the
next tick, narrowly: rewrite (or add alongside) the `protocol_version_is_three`
test in `crates/rylus-core/src/protocol.rs` so it actually reads
`docs/PROTOCOL.md` (e.g. `include_str!("../../../docs/PROTOCOL.md")` from
the crate, or a workspace-relative path via `CARGO_MANIFEST_DIR`) and
asserts the doc's stated `PROTOCOL_VERSION` / `MIN_CLIENT_PROTOCOL_VERSION`
values (e.g. via a regex/substring check against the doc's `pub const
PROTOCOL_VERSION: u32 = 3;` code block and the prose `MIN_CLIENT_PROTOCOL_VERSION
= 2` in §1.2) actually match the real constants in code, not a
second hardcoded literal. Re-verify by bumping `PROTOCOL_VERSION` locally
in a scratch edit (not committed) and confirming the new test fails when
the doc isn't updated, then revert the scratch edit — that's the real
proof it's wired, not just green once. Everything else audited this tick
(all 5 gates, self-test substance, mdns fix stability, bench harness
reality, CI wiring) is confirmed solid and does not need re-auditing next
tick unless touched.

## 2026-07-20 tick — REMEDIATION: both AUDIT findings fixed, back to AUDIT

Fixed exactly the two blocking issues from the prior AUDIT:FAIL tick, nothing
else. Dispatched 2 disjoint sonnet workers in their own worktrees
(`scripts/worktree.sh create`), independently re-verified each on my own side
before integrating (never trusted the self-reports alone), then integrated
both onto `integration` via `scripts/integrate.sh` with real gate commands.

**1. mdns race (`collision_suffix()`, `crates/rylus-server/src/mdns.rs`).**
`concern/fix-mdns-race` → `40203ca`, integrated `3f5c073`. Replaced the
non-atomic load-then-`store` on the cached `AtomicU32` with a single
`compare_exchange(0, mixed, Relaxed, Relaxed)`, so the cache populates
exactly once and every caller (winner or loser) converges on the same
value. Diff is 4 lines, only that one function touched. Independently
re-ran `cargo test -p rylus-server mdns:: --locked -- --test-threads=8` 10x
back-to-back on my own side twice — once in the worker's worktree pre-merge,
once again on the integrated `integration` tip post-merge — **20/20 clean,
zero flakes**. Not a lucky roll: this closes the actual data race, not just
reduces its probability. `M1.P9.S1.T1`'s ledger entry (artifact `cargo test
--workspace --locked`) is left as-is per the remediation brief (command
unchanged, now genuinely stable) — re-ran the full workspace suite 3x more
this tick, all exit 0, zero failures (67/67 in `rylus-server`'s own unit
binary each time, no `FAILED` lines anywhere).

**2. `M1.P1.S1.T3` vacuous artifact (`crates/rylus-server/src/self_test.rs`).**
`concern/fix-selftest-test` → `4989af3`, integrated `170c509`. Added a
`#[cfg(test)] mod tests` block at the end of the file (the crate has no
`[lib]` target, so a `tests/*.rs` integration file can't link against
`self_test::run` — matched the existing bin-only architecture rather than
inventing one) with one test, `self_test_run_passes_and_tears_down_cleanly`,
that calls `run()` in-process and asserts `true`. Independently re-ran
`cargo test -p rylus-server self_test --locked -- --nocapture` myself both
pre- and post-merge: `test result: ok. 1 passed; 0 failed` both times (was
"running 0 tests" before). Retracted the prior false-positive ledger record
and re-recorded for real: `ledger.py kill --todo M1.P1.S1.T3 --reason
"artifact was vacuous — 0 tests matched, no test existed"` then `ledger.py
done --todo M1.P1.S1.T3 --cmd 'cargo test -p rylus-server self_test
--locked' --commit 170c509 --concern self-test-integration-test --run` (the
`--run` flag executed the command for real as part of recording).

**One self-caused regression, caught and fixed before declaring done:** the
new test assertion line in `self_test.rs` exceeded rustfmt's width and broke
M1G3 (`cargo fmt -- --check`, `M1.P9.S1.T3`) — `ledger.py check --rerun`
caught it immediately after integrating (`FAIL: M1.P9.S1.T3 RERUN 'cargo fmt
-- --check' => 1`). Ran `cargo fmt -p rylus-server` and committed the
4-line reformat separately (`b94eeaa`, `chore: cargo fmt self_test.rs`).
Re-ran `cargo fmt -- --check` after: clean. This is why "green once" isn't
enough — re-verify after every change, including your own follow-ups.

**Final re-check before flipping the control block:** `ledger.py check
--rerun` → PASS (15/15 done todos, structural+rerun). `cargo fmt -- --check`
→ clean. `cargo test --workspace --locked` run 3 additional times → exit 0
all 3, zero `FAILED` lines. Both audit findings are closed with real
evidence, not self-report. Flipping `MILESTONE_PHASE` to `AUDIT`,
`CURRENT_MILESTONE` stays `M1` — this tick does not audit its own work, a
future tick re-verifies and decides master fast-forward.

## 2026-07-20 tick — AUDIT: FAIL (2 blocking issues, master NOT fast-forwarded)

Independent re-verification of M1 against its 5 gates. Not a rubber stamp of
the prior tick's self-report — reran everything from repo root on
`integration` tip (`9ecaedf`). Two blocking findings, both real, neither
cosmetic:

**1. M1G1 (`cargo test --workspace --locked`) is FLAKY, not green.** Ran the
exact gate command 3x back-to-back: FAILED once
(`mdns::tests::collision_suffix_is_stable_across_calls`, left:"hntu"
right:"dw0v"), passed twice. Root cause is a real data race in production
code, not test flakiness: `collision_suffix()`
(`crates/rylus-server/src/mdns.rs:128-145`) does a non-atomic
load-check-then-store on a `static AtomicU32` with `Relaxed` ordering — when
`cargo test` runs multiple test threads in the same binary concurrently, two
threads can both observe `CACHED == 0`, each compute a different mixed value
from PID+timestamp jitter, and race to store, so a single test's two
sequential calls to `collision_suffix()` can observe different values
depending on interleaving. This is exactly the bug the test's own doc
comment (`"Stable for the lifetime of the process"`) says must not happen.
`ledger.py check --rerun` happened to land on a passing interleaving both
times I invoked it, and reported `ledger check: PASS (15 done todos,
structural+rerun)` — a lucky roll, not a verified-stable gate. This blocks
M1G1 and, by extension, `M1.P9.S1.T1`.

**2. `M1.P1.S1.T3`'s artifact is a false positive — no test exists.** The
todo requires "an integration test that invokes the self-test path and
asserts a clean exit and teardown," artifact-checked by
`cargo test -p rylus-server self_test`. `crates/rylus-server/src/self_test.rs`
(236 lines) contains only the routine itself — zero `#[test]` functions,
no `#[cfg(test)] mod tests`, no `tests/` integration file referencing it.
Running the literal artifact command confirms: `cargo test -p rylus-server
self_test` → "running 0 tests" → exit 0. The check command is a filter
against a name substring; when nothing matches, `cargo test` still exits 0,
so the ledger recorded `{"verified_by":{"cmd":"cargo test -p rylus-server
self_test","exit":0}}` (`ledger.jsonl` line 14, 2026-07-20T19:22:26-0400) as
proof of a test that was never written. This is a placeholder marked DONE
via a vacuously-true shell check, not a wired safeguard.

Everything else checked out clean — recording for the remediation tick so it
isn't re-audited from scratch:
- M1G2 `cargo clippy --all-targets -- -D warnings` → PASS, no warnings.
- M1G3 `cargo fmt -- --check` → PASS, no diff.
- M1G4 `cargo run -q -p rylus-server -- --self-test` → PASS, exit 0, real
  capture(testsrc)→encode(libx264, full GOP)→bind→WS-accept→teardown path
  observed in logs, not a stub.
- M1G5 `test -f docs/PROTOCOL.md && grep -q HeartbeatAck ...` → PASS.
- `docs/PROTOCOL.md` diffed by hand against every variant in
  `crates/rylus-core/src/protocol.rs`: all 14 `MessageInbound` variants and
  all 10 `MessageOutbound` variants are individually documented (§3.1–3.14,
  §4.1–4.2.7) with accurate JSON shapes, including the one serde-rename
  exception (`batched_pointer_events`) and the exact field tables for
  `PointerEvent`/`KeyboardEvent`. No drift found. Consistent with ADR-0003
  (LAN-only, WebSocket-only v1 transport) — single-WebSocket framing (text
  control / binary fMP4), no alternate transport implied anywhere in the doc.
- `crates/rylus-encode/benches/encode.rs` is a real criterion bench
  (`VideoEncoder::encode()` over a real libx264 software path, not a stub)
  and `BASELINE.md` records a real measured run (668µs mean, host/FFmpeg
  version stated) rather than placeholder numbers. Re-ran
  `scripts/bench-gate.sh` directly: it re-executes the bench for real,
  parses criterion's actual stdout, computes delta against the baseline
  parsed out of `BASELINE.md` (not a hardcoded duplicate number), and would
  exit non-zero past a 15% regression threshold. Measured this run: 678.6µs,
  +1.6% vs baseline, PASS. This gate is functionally real, not cosmetic.
- `.github/workflows/build.yml` `quality` job: `Self-test` step
  (`cargo run -q -p rylus-server -- --self-test`, line 37-38) and
  `Encode benchmark regression gate` step (`./scripts/bench-gate.sh`, line
  39-40) are both real steps in a job every other job `needs:`, so a
  non-zero exit from either genuinely fails the pipeline — not grep-only
  cosmetic wiring.
- `git log --oneline -20` on `integration`: commits are atomic, one concern
  each, and match STATE.md's tick narrative (self-test-flag → protocol-doc →
  encode-bench → the 3 salvaged orphans → ci-selftest-step →
  gui-clippy-f32). No surprises.
- ADR reference check: the audit brief cited "ADR-0001" for the
  WebSocket-only v1 decision; the actual repo has that decision in
  **ADR-0003** (`docs/adr/ADR-0003-lan-only-websocket-transport-v1.md`) —
  ADR-0001 here is the generic "record architecture decisions" meta-ADR.
  Noting the mismatch for whoever sourced that number; not itself a finding
  against M1, and the substance (protocol docs/self-test matching the
  WebSocket-only contract) checks out either way.

**Verdict: FAIL.** Did not touch `master`. `MILESTONE_PHASE` set to
`REMEDIATION`, `CURRENT_MILESTONE` stays `M1`. Remediation scope for the
next tick: (a) fix `collision_suffix()` to use a compare-and-swap
(`compare_exchange`) instead of load-then-store so the cache populates
exactly once under concurrent callers, and re-run M1G1 several times to
confirm the flake is actually gone, not just less likely; (b) write the
missing `M1.P1.S1.T3` integration test for real (invoke the self-test path,
assert exit and no leaked threads/devices), then re-verify the artifact
command against a nonzero passed-test count, not just exit code 0.

- 2026-07-20T22:35:30Z — integrated `concern/self-test-flag` into `integration` at `024080e`
- 2026-07-20T22:37:52Z — integrated `concern/protocol-doc` into `integration` at `f498d1c`
- 2026-07-20T22:38:38Z — integrated `concern/encode-bench` into `integration` at `2738d4f`
- 2026-07-20T23:14:30Z — integrated `concern/fmt-fix` into `integration` at `764d89e`
- 2026-07-20T23:14:32Z — integrated `concern/bench-ci-gate` into `integration` at `afb88c2`
- 2026-07-20T23:14:40Z — integrated `concern/self-test-routine` into `integration` at `281b4df`
- 2026-07-20T23:21:17Z — integrated `concern/ci-selftest-step` into `integration` at `8b198c5`
- 2026-07-20T23:21:20Z — integrated `concern/gui-clippy-f32` into `integration` at `8dc5a49`
- 2026-07-20T23:36:44Z — integrated `concern/fix-mdns-race` into `integration` at `40203ca`
- 2026-07-20T23:37:49Z — integrated `concern/fix-selftest-test` into `integration` at `4989af3`
- 2026-07-21T00:10:39Z — integrated `concern/origin-test` into `integration` at `eb7bec4`
- 2026-07-21T00:12:02Z — integrated `concern/security-review-doc` into `integration` at `1ab1142`
- 2026-07-21T00:12:23Z — integrated `concern/axe-a11y-wiring` into `integration` at `a60068b`
- 2026-07-21T00:27:20Z — integrated `concern/cargo-audit-clean` into `integration` at `2ffc321`
- 2026-07-21T00:35:51Z — integrated `concern/a11y-violations-fixed` into `integration` at `053b9a7`
- 2026-07-21T00:41:16Z — integrated `concern/latency-instrumentation` into `integration` at `84f2059`
- 2026-08-13T17:47:27Z — integrated `concern/macos-icon-icns` into `integration` at `ba5e949`
- 2026-08-13T17:47:29Z — integrated `concern/macos-entitlements` into `integration` at `6ffa467`
- 2026-08-13T17:47:31Z — integrated `concern/macos-bundle-metadata` into `integration` at `a35e240`
- 2026-08-13T19:21:36Z — integrated `concern/macos-universal-ci` into `integration` at `d61455a`
- 2026-08-13T19:21:38Z — integrated `concern/readme-dmg-install` into `integration` at `8df53f8`
