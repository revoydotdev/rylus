# Swarm State Ledger

> Append-only-ish ledger the supervisor maintains each tick. Keep newest at top.
> Status keys: CLAIMED · IN_PROGRESS · DONE · BLOCKED · GATE-FAILED

<!-- CONTROL: machine-read; supervisor updates these two lines -->
- MILESTONE_PHASE: NORMAL
- CURRENT_MILESTONE: M1

## 2026-07-20 tick — self-heals + 3 new concerns

Self-heal pass (`scripts/self-heal-check.sh <id> --cmd '<cmd>' --record`), all
4 candidates verified PASS and recorded for free (no worker, no new commit
content beyond ledger bookkeeping):
- `M1.P3.S1.T2` — `crates/rylus-encode/benches/BASELINE.md` already exists (landed with `encode-bench`).
- `M1.P2.S1.T2` — `docs/PROTOCOL.md` already contains `HeartbeatAck`/`RequestKeyframe`/`HelloNack` (landed with `protocol-doc`).
- `M1.P9.S2.T2` (closes gate M1G5) — same doc satisfies the gate-level check too.
- `M1.P2.S1.T3` — `cargo test -p rylus-core protocol_version` passes (`protocol_version_is_three` test already present).

CLAIMED (3 disjoint concerns, dispatched to sonnet workers):
- CLAIMED `M1.P1.S1.T2` — concern `self-test-routine` — implement the real `--self-test` routine (touches `crates/rylus-server/src/{main,rylus}.rs`, possibly `crates/rylus-core/src/config.rs`)
- CLAIMED `M1.P9.S1.T3` — concern `fmt-fix` — mechanical `cargo fmt` fix (touches `crates/rylus-capture/src/{captrs_capture,x11}.rs`, `crates/rylus-encode/src/lib.rs`)
- CLAIMED `M1.P3.S2.T1` — concern `bench-ci-gate` — wire a real bench regression gate into `.github/workflows/build.yml`

## 2026-07-20 tick — gate decomposition + M1 concerns

First NORMAL tick with 0 done M1 todos (`ledger.py status --milestone M1`).
Gate-decomposed M1.P9 (bare M1G1–M1G5 bullets) into `M1.P9.S1`/`M1.P9.S2`
`T#` todos in ROADMAP.md, same ID/artifact-check style as M1.P1–P3:
- `M1.P9.S1.T1` → M1G1 · `cargo test --workspace --locked`
- `M1.P9.S1.T2` → M1G2 · `cargo clippy --all-targets -- -D warnings`
- `M1.P9.S1.T3` → M1G3 · `cargo fmt -- --check`
- `M1.P9.S2.T1` → M1G4 · `cargo run -q -p rylus-server -- --self-test`
- `M1.P9.S2.T2` → M1G5 · `test -f docs/PROTOCOL.md && grep -q 'HeartbeatAck' docs/PROTOCOL.md`

CLAIMED (3 disjoint concerns, feature todos, dispatched to sonnet workers):
- CLAIMED `M1.P1.S1.T1` — concern `self-test-flag` — add `--self-test` clap flag to `rylus-server` (touches `crates/rylus-server/src/{main,rylus}.rs`, `crates/rylus-core/src/config.rs`)
- CLAIMED `M1.P2.S1.T1` — concern `protocol-doc` — write `docs/PROTOCOL.md` from `crates/rylus-core/src/protocol.rs`
- CLAIMED `M1.P3.S1.T1` — concern `encode-bench` — add criterion bench scaffold under `crates/rylus-encode/benches/`

DONE — `M1.P1.S1.T1` — integrated `concern/self-test-flag` at `024080e` (ff-merge `950d7bf`) — verified `cargo run -q -p rylus-server -- --help | grep -q -- '--self-test'` exit 0.
DONE — `M1.P2.S1.T1` — integrated `concern/protocol-doc` at `f498d1c` (ff-merge `49d3e05`) — verified `test -f docs/PROTOCOL.md && grep -q MessageInbound ... && grep -q MessageOutbound ...` exit 0 (doc also independently satisfies the stronger HeartbeatAck/RequestKeyframe/HelloNack check, i.e. M1G5/M1.P9.S2.T2's check, though only M1.P2.S1.T1 is recorded done this tick).
DONE — `M1.P3.S1.T1` — integrated `concern/encode-bench` at `2738d4f` (ff-merge `d892d55`) — verified `cargo bench -p rylus-encode --no-run` exit 0, and independently re-ran the real bench (`cargo bench -p rylus-encode`) on the integrated tree to confirm it exercises the actual `VideoEncoder::encode()`/libx264 path (not a stub): ~687µs/frame, matching the ~668µs baseline recorded in `crates/rylus-encode/benches/BASELINE.md`.

3 of 15 M1 todos done this tick (bounded to 3 concerns per protocol); `ledger.py next --milestone M1` shows 12 remaining, so `MILESTONE_PHASE` stays `NORMAL` (not AUDIT-eligible). Known gaps for a future tick: `cargo fmt -- --check` (M1G3/`M1.P9.S1.T3`) already fails on pre-existing drift in `rylus-capture` (`captrs_capture.rs`, `x11.rs`) and `rylus-encode/src/lib.rs`, unrelated to this tick's changes — not fixed here (out of scope for the 3 claimed concerns; surgical-change discipline). Full `cargo test --workspace --locked` / `cargo clippy --all-targets -- -D warnings` (M1G1/M1G2) not re-run in full this tick (only the touched crates' scoped tests/builds were verified per concern) — a future M1.P9.S1 tick should run and record these for real.

One process note for the harness: `scripts/integrate.sh`'s failure path does `git reset --hard` to the pre-rebase concern SHA, which silently discards any uncommitted changes on the branch being integrated (not just failed-rebase state) — including unrelated uncommitted edits sitting on the target branch when integrate.sh switches away from it. Lost one `ledger.py done --run` recording this way (redone before the second integrate.sh attempt). Workaround used for the rest of this tick: commit ledger.jsonl + STATE.md immediately after each `ledger.py done`, before invoking `integrate.sh` for the next concern, so there is never uncommitted state at risk. Also: `worktree.sh destroy` cannot run after a worker's branch is still checked out in its own worktree at the point `integrate.sh` needs to check that branch out into the main worktree (git forbids the same branch checked out twice) — worked around this tick via `git worktree remove --force <dest>` (keeps the branch, just frees the worktree lock) ahead of `integrate.sh`, then `git branch -d concern/<tag>` after integration instead of the full `worktree.sh destroy` (which requires the worktree dir to still exist). Worth revisiting `worktree.sh`/`integrate.sh`'s expected call order in a future tick.

## 2026-07-20 tick — preflight DIRTY, skipped
`scripts/preflight.sh` → `DIRTY:2 files` (untracked `STATUS.md`, `.studio/` —
pre-existing local artifacts from a prior manual `/studio` session, not
tick-lock residue; neither is in `.gitignore` despite `STATUS.md`'s own header
claiming it is). `worktree-check.sh` clean (no orphaned `concern/*`
worktrees/branches/salvage); `integration` is a clean 5-commit fast-forward
ahead of `master`, nothing to reconcile. Per protocol, left the untracked
files untouched (not tick residue, not mine to clobber) and skipped NORMAL
work this tick. Next tick: if these are meant to stay untracked, someone
should add them to `.gitignore`; if they're stale, remove manually.

## enrollment
Scaffolded into the swarm by `enroll.py` (ADR-0028). Awaiting its first tick.
- 2026-07-20T22:35:30Z — integrated `concern/self-test-flag` into `integration` at `024080e`
- 2026-07-20T22:37:52Z — integrated `concern/protocol-doc` into `integration` at `f498d1c`
- 2026-07-20T22:38:38Z — integrated `concern/encode-bench` into `integration` at `2738d4f`
