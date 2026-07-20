# Swarm State Ledger

> Append-only-ish ledger the supervisor maintains each tick. Keep newest at top.
> Status keys: CLAIMED · IN_PROGRESS · DONE · BLOCKED · GATE-FAILED

<!-- CONTROL: machine-read; supervisor updates these two lines -->
- MILESTONE_PHASE: NORMAL
- CURRENT_MILESTONE: M1

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
