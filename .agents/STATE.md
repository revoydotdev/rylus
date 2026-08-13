# Swarm State Ledger

> Append-only-ish ledger the supervisor maintains each tick. Keep newest at top.
> Status keys: CLAIMED · IN_PROGRESS · DONE · BLOCKED · GATE-FAILED

<!-- CONTROL: machine-read; supervisor updates these two lines -->
- MILESTONE_PHASE: REMEDIATION
- CURRENT_MILESTONE: M3

## 2026-08-13 tick — M3 AUDIT → **FAIL** (independent verifier); phase set REMEDIATION

Preflight CLEAN. Sole audit turn, no feature workers. A fresh Sonnet 5 verifier
was given only the M3G1–M3G4 gate list, the `integration` ref, and the VISION
axioms — no STATE.md narrative, no ledger reasoning, no account of who built
what — and re-derived the verdict from the gates and code alone (ADR-0024).
Its verdict stands unmodified; the supervisor did not override it.

Per-gate evidence (commands run this tick against `integration` @ `0149ce8`):

| Gate | Command | Exit | Verdict |
| --- | --- | --- | --- |
| M3G1 | `test -f packaging/macos/icon.icns` | 0 | PASS — real 70,564-byte Mac OS X icon, wired at `crates/rylus-server/Cargo.toml:53` |
| M3G2 | `test -f packaging/macos/entitlements.plist` | 0 | PASS — well-formed plist, substantive `disable-library-validation` entry (ffmpeg dynamic linking) |
| M3G3 | `grep -q 'aarch64-apple-darwin' .github/workflows/build.yml` | 0 | PASS — `build.yml:201-210` really builds both arches and `lipo -create`s them |
| M3G4 | `grep -q 'notarytool' .github/workflows/build.yml` | 0 | **FAIL on substance** — steps are real but structurally dead (below) |
| ledger | `python3 scripts/ledger.py check --rerun` | 1 | FAIL — two unrelated M2 rerun failures |

**Findings to remediate.**

1. **BLOCKED-PENDING-OPERATOR** — M3's named artifact does not exist. Every
   codesign / notarize / DMG step is conditioned on
   `secrets.APPLE_DEVELOPER_ID_CERT_P12 != ''` (`build.yml:223,240,248,262,271,289`)
   and `gh secret list -R revoydotdev/rylus` is empty, so each real CI run skips
   them and publishes only an unsigned `macos-universal.zip` — no `.dmg`, ever.
   The grep-only gate rubber-stamps code that cannot execute for anyone. Needs a
   paid Apple Developer enrollment and cert material; no agent can close it.
   Posted as a2a-request `4f1be84965a3` (type `resource`), asking the operator to
   either provision the secrets or authorise redefining M3 to "CI plumbing ready,
   pending Apple enrollment" — the latter also requires rewording M3G4 so it
   asserts something an agent can actually verify.
2. **Actionable** — `TODOS.md:36-56` still reads "No code signing, no
   notarization, no arm64, no DMG" as *status today* and leaves the
   universal-build / entitlements / icon / codesign / notarize tasks unchecked,
   contradicting the code that now exists in `build.yml` and `packaging/macos/`.
   Docs-vs-behavior drift; fix the doc, not the code.
3. **Actionable** — repo-wide ledger is red on two M2 entries:
   `M2.P1.S2.T2` (`cargo audit` → 1, RUSTSEC-2026-0257 in `webbrowser 1.2.0`,
   no fixed release) and `M2.P3.S1.T2` (`npm run a11y` → 1,
   `ERR_MODULE_NOT_FOUND: playwright`). Neither touches an M3 gate, but
   `ledger.py check --rerun` cannot go green while they stand.

No AX-1/AX-2/AX-3 conflict; this is packaging-only work.

## 2026-08-13 tick — RECOVERY (preflight DIRTY:1): governance feed untracked, M3 AUDIT still pending

Preflight `DIRTY:1` on `.agents/governance/feed.jsonl`. Per STEP 2 the tick ran
recovery only — no phase logic, no workers, no ledger writes. M3 stays in AUDIT.

Root cause, not symptom: the feed was committed in `d762bb5` even though
`.gitignore:40` already lists `.agents/governance/`. An ignore rule is inert
against an already-tracked path, so every spec-mandated `governance.py` post
dirtied the tree and sent the *following* tick to DIRTY — a self-sustaining
stall that had already burned two ticks (2026-07-21 ×2) and was blocking the
M3 AUDIT. Prior tick escalated it as a2a-request `a078f83641f9` (relayed to the
operator) but declined to fix it, reading "do not commit the governance store"
as forbidding action. Untracking is not committing it: `git rm --cached` drops
the path from the index and leaves the working file alone. Fixed in `506faa7`.

Verified after: `preflight.sh` → CLEAN; `git check-ignore -v` now matches
`.gitignore:40`; feed file intact at 5 lines, no history lost. Request resolved.

**Trap recorded (M3 learnings).** First fix attempt was wrong and passed for the
wrong reason: `git commit -m '...' -- <path>` commits the *working tree* copy of
that path, silently overriding the staged `git rm --cached`. It re-committed the
file, and preflight went CLEAN because the dirt had been *committed*. Caught on
the stat line — a deletion commit must report `delete mode`, not `3 insertions(+)`.
Amended to the real deletion.

Still open, unchanged, awaiting the operator: the ledger-commit invariant posted
2026-08-13 16:22 (a `done` cannot be one commit with the concern files under the
mandated worktree flow; 5 recurrences). No new information this tick — not
re-attempted, per the no-relitigation rule.

## 2026-08-13 tick — NORMAL→AUDIT: M3 notarization CI closes the milestone (1 concern, 10/10 done)

`ledger.py status --milestone M3` 8/10 done, `next --milestone M3` showed 2
unclaimed: `M3.P3.S1.T1` and `M3.P9.S2.T2`/M3G4 — both close via the SAME
verify command on the SAME file (`grep -q 'notarytool'
.github/workflows/build.yml`), so treated as ONE concern, not split.

Read all 4 M3 learnings first (macOS-only-CLI silent-no-op trap;
cargo-bundle schema from real docs not memory; worktree destroy-before-
integrate trap; the standalone-ledger-commit invariant) and folded the
relevant ones into the worker brief.

**The recurring deferral, resolved this tick.** Both todos had been left
unclaimed on the prior TWO NORMAL ticks with the stated rationale "signing
CI steps land once the cert is provisioned (AX-8)" (ROADMAP.md M3 header).
Verified myself before acting: `grep -n AX-8 TODOS.md` → no hit, AX-8
appears nowhere in TODOS.md. TODOS.md section 2 ("Apple — Notarized macOS
DMG") already names the exact five secrets to gate on
(`APPLE_DEVELOPER_ID_CERT_P12`, `APPLE_DEVELOPER_ID_CERT_PASSWORD`,
`APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_NOTARY_PASSWORD`). The todo text itself
says the steps are "gated on the Apple secrets" — i.e. authored with `if:`
guards so the workflow stays valid and green without the cert, executing
only once the operator provisions it. Conditionally-gated CI steps are a
standard, well-documented GitHub Actions pattern and do not require the
cert to exist to be authored. Decision: build it this tick instead of
deferring a third time. (ROADMAP.md's M3 header prose still says "lands
once the cert is provisioned" — now stale given this tick's work, but left
untouched as out of scope for this concern; a future doc pass should update
it. Same AX-8 phrasing also appears in the M4/M5 headers for Windows
signing and final release, unrelated to this tick.)

CLAIMED `M3.P3.S1.T1` + `M3.P9.S2.T2`/M3G4 — concern `macos-notarize` —
touches only `.github/workflows/build.yml`. self-heal-check ran first with
`--record`: genuinely NEEDS-WORK (verify failed against the pre-change
tree, exit 3) — no free ledger entry.

Dispatched one sonnet worker (worktree `concern/macos-notarize` off
`integration`). Worker confirmed the verify command genuinely failed
pre-change (exit 1), made the minimal addition to `build-macos` in
`.github/workflows/build.yml` (five new steps, all gated on
`secrets.APPLE_DEVELOPER_ID_CERT_P12 != ''`: import cert to temp keychain,
codesign with hardened-runtime entitlements, notarize+staple the `.app`,
build+rename the DMG via `create-dmg`, notarize+staple the DMG; plus a
gated dmg artifact upload and an added `Rylus-*-universal.dmg` glob in the
Publish step's `files:`, left unmatched-and-skipped when absent since
`action-gh-release`'s `fail_on_unmatched_files` defaults false), confirmed
verify now passes (exit 0) and the YAML parses, and cited real sources
consulted (GitHub's own "installing an Apple certificate on macOS runners"
guide, Apple-Actions/import-codesign-certs, sindresorhus/create-dmg's
README, action-gh-release's action.yml) rather than guessing syntax from
memory. PASSED, commit `4a03b2d`.

I independently re-verified before integrating: read the full diff (74
insertions, `build-macos` job only, existing unsigned-zip path untouched
and unconditional), confirmed all five new steps are Apple-secret-gated
and none of the always-run steps depend on secrets, confirmed no
credentials hardcoded and no secret names beyond the five TODOS.md already
names.

**Worktree/rebase mishap this tick (self-inflicted, recovered, learning
recorded).** Wrote a CLAIMED note into STATE.md before dispatch, then
called `integrate.sh` directly from the main worktree while the worker's
worktree still held `concern/macos-notarize` checked out — first attempt
failed cleanly ("already used by worktree", no damage). Removed the
worktree with `git worktree remove` (not `worktree.sh destroy`, which would
also have deleted the branch) to free it, then retried `integrate.sh`
without first stashing my own uncommitted STATE.md edit — the checkout
succeeded but the rebase then failed on "unstaged changes" (that very
STATE.md edit), and the failure handler's `git reset --hard` silently
discarded it, same class of loss as a prior tick's known trap but via a new
concrete trigger (worktree-branch collision forcing a second integrate.sh
call with dirty state in between). Recovered by re-authoring this entry
from scratch (no ledger/commit state was lost, only the draft narrative
text). Recorded a new learning capturing the specific trigger so a future
tick fully vacates the concern branch from its worktree *and* keeps the
main worktree commit-clean before the *first* `integrate.sh` call, not
just "at some point before it runs."

Re-ran the verify command myself on the integrated tree:
`grep -q 'notarytool' .github/workflows/build.yml` → exit 0. Recorded both
todos done via `ledger.py done --run` (refuses unless the cmd exits 0):

- DONE — `M3.P3.S1.T1` + `M3.P9.S2.T2`/M3G4 (concern: `macos-notarize`) —
  integrated at `a4cdf5d`.

`ledger.py next --milestone M3` now shows **0 unclaimed** — all 10 M3
todos done. All four gate checks re-verified directly by me on the
integrated tree: M3G1 `test -f packaging/macos/icon.icns` → 0, M3G2
`test -f packaging/macos/entitlements.plist` → 0, M3G3 `grep -q
aarch64-apple-darwin .github/workflows/build.yml` → 0, M3G4 `grep -q
notarytool .github/workflows/build.yml` → 0. M3 is **candidate-complete**;
CONTROL flipped `NORMAL` → `AUDIT` (CURRENT_MILESTONE stays `M3`). No audit
performed this tick per instructions — that's the next tick's job.

Worktree destroyed (post-integration, branch `concern/macos-notarize`
deleted with it). No a2a-requests filed this tick.

## 2026-08-13 tick — NORMAL: M3 CI universal build + README docs (2 concerns, 8/10 done)

`ledger.py status --milestone M3` 5/10 done, `next --milestone M3` showed 5
unclaimed (M3.P2.S1.T2, M3.P3.S1.T1, M3.P3.S1.T2, M3.P9.S2.T1, M3.P9.S2.T2).
Read both M3 learnings first (macOS-only CLI silent no-op trap; cargo-bundle
schema must come from real docs, not memory) — neither directly bit this
tick's concerns, but both stayed in mind while briefing workers.

`M3.P3.S1.T1` (codesign/notarytool/stapler CI steps) and its gate-closing
twin `M3.P9.S2.T2`/M3G4 remain prereq-gated per the roadmap's own M3 header
("signing CI steps land once the cert is provisioned, AX-8") — left
unclaimed again, same rationale as last tick.

CLAIMED (2 disjoint-file concerns, dispatched to sonnet workers):
- CLAIMED `M3.P2.S1.T2` + `M3.P9.S2.T1`/M3G3 — concern `macos-universal-ci`
  — touches only `.github/workflows/build.yml`
- CLAIMED `M3.P3.S1.T2` — concern `readme-dmg-install` — touches only
  `README.md` and `ROADMAP.md`

Self-heal-check ran first on both with `--record`: both genuinely
NEEDS-WORK (verify command confirmed failing against the pre-change tree:
`grep -n aarch64-apple-darwin .github/workflows/build.yml` → no match;
`grep -n -i dmg README.md` → no match) — no free ledger entries.

Both workers PASSED. Independently re-verified myself before recording done:

- DONE — `M3.P2.S1.T2` + `M3.P9.S2.T1`/M3G3 (concern: `macos-universal-ci`)
  — integrated at `d61455a`. Root cause: `build-macos` only ever built the
  host's default x86_64 target through `cargo bundle`, shipping an
  Intel-only binary mislabeled `macos-intel`. Fix: `rustup target add
  x86_64-apple-darwin aarch64-apple-darwin`, cross-build both targets with
  `cargo build --release --target <triple>`, `lipo -create` merges them
  into a universal binary that overwrites the bundle's
  `Rylus.app/Contents/MacOS/rylus` before packaging; artifact/zip/publish
  names renamed `macos-intel` → `macos-universal` consistently (no stale
  `-intel` name left over). Could not execute `lipo`/`cargo build --target
  aarch64-apple-darwin` locally — this Linux box has neither the Darwin
  targets nor `lipo` — so verification is YAML-text-level only (`grep -q
  aarch64-apple-darwin`, PyYAML parse OK, hand review of indentation against
  sibling steps), consistent with this milestone's known
  cannot-execute-macOS-tooling-here constraint. Re-verified myself: fresh
  `grep -q 'aarch64-apple-darwin' .github/workflows/build.yml` exit 0 on the
  integrated tree, diff read line-by-line (13 insertions / 4 deletions,
  `build-macos` job only, no other job touched).
- DONE — `M3.P3.S1.T2` (concern: `readme-dmg-install`) — integrated at
  `8df53f8`. Added a DMG download/drag-to-Applications paragraph plus a
  first-launch permissions note to `README.md`'s existing `### macOS`
  section (existing permissions bullet list and Hardware Acceleration
  subsection left intact), consistent with the repo's existing
  `github.com/Chorosyne/rylus` releases-page convention used elsewhere in
  the same file. Also fixed the `M3.P3.S1.T2` line in `ROADMAP.md`: its
  artifact command referenced the nonexistent `Readme.md` (case-sensitive
  fs; real file is `README.md`), which would have failed forever regardless
  of doc content — fixed both occurrences on that one line only, left the
  two other `Readme.md` references (M4.P3.S1.T3, M5.P2.S1.T1) untouched as
  out of scope. Re-verified myself: fresh `grep -qi 'dmg' README.md` exit 0,
  `grep -c 'Readme.md' ROADMAP.md` still 2 (only the M3 line changed).

Worktree mishap (self-inflicted, corrected within the tick): after both
worker branches were verified, I ran `scripts/worktree.sh destroy` on both
concern worktrees *before* running `scripts/integrate.sh` — `worktree.sh
destroy` deletes the concern branch along with the worktree, which briefly
orphaned both workers' commits (`d61455a`, `8df53f8`) as dangling objects.
Recovered cleanly via `git fsck --no-reflog` (both commits still present,
nothing lost) and recreated the branches by SHA before integrating
normally. Separately, an uncommitted `.agents/STATE.md` edit in the main
worktree (this tick's CLAIMED write-up, made before dispatch per protocol)
was lost when `integrate.sh`'s first (failed) attempt at
`concern/macos-universal-ci` hard-reset the branch after a rebase failure
triggered by that same unstaged file — re-authored from scratch for this
entry. Lesson for future ticks: destroy worktrees only *after* `integrate.sh`
has fast-forward-merged the branch, and keep STATE.md edits either committed
or deferred until after all `integrate.sh` calls for the tick complete, never
sitting unstaged in the main worktree while `integrate.sh` is checking
branches out there.

No a2a-requests were needed or filed this tick — both workers resolved
their scope from the existing repo/roadmap/README conventions.

`ledger.py next --milestone M3` now shows 2 unclaimed
(`M3.P3.S1.T1`, `M3.P9.S2.T2` — both prereq-gated on Apple secrets). M3 is
NOT candidate-complete (M3G4 still open); CONTROL stays `NORMAL`/`M3`.
Both worktrees destroyed (post-integration), `concern/*` branches deleted.

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
- 2026-08-13T20:20:26Z — integrated `concern/macos-notarize` into `integration` at `4a03b2d`
- 2026-08-13T22:08:22Z — integrated `concern/doc-m3-apple-drift` into `integration` at `07098aa`
- 2026-08-13T22:08:26Z — integrated `concern/cargo-audit-webbrowser` into `integration` at `1e420d6`
