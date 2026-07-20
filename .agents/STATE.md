# Swarm State Ledger

> Append-only-ish ledger the supervisor maintains each tick. Keep newest at top.
> Status keys: CLAIMED · IN_PROGRESS · DONE · BLOCKED · GATE-FAILED

<!-- CONTROL: machine-read; supervisor updates these two lines -->
- MILESTONE_PHASE: NORMAL
- CURRENT_MILESTONE: M1

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
