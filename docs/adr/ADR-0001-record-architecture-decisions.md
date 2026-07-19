# ADR-0001 — Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-07-19

## Context

Rylus is a solo-maintained fork of Weylus that has diverged substantially: the
entire codebase was rewritten in Rust, the capture, GUI, and FFI layers were
replaced, and a security layer was added for network-exposed operation. As the
project moves toward a 1.0.0 release and toward swarm-assisted development, the
*reasons* behind its load-bearing decisions must survive in the repository
rather than in one person's memory or in transient session notes. A future
contributor (human or agent) needs to know not just what the architecture is,
but why it is that way and what alternatives were rejected.

## Decision

We record significant, durable, or surprising architectural decisions as
Architecture Decision Records, following Michael Nygard's lightweight format.

- ADRs live in `docs/adr/`, one file per decision, named
  `ADR-NNNN-short-title.md` with a zero-padded sequential number.
- Each ADR captures a single decision with the sections **Context**,
  **Decision**, and **Consequences**, plus a title, status, and date.
- ADRs record the roads not taken and why, not only the chosen path.
- Once **Accepted**, an ADR is immutable. If a decision changes, a new ADR is
  written that supersedes the old one; the old one is marked **Superseded by
  ADR-NNNN** rather than edited or deleted.
- Every ADR must be consistent with `VISION.md`, or must explicitly supersede
  the axiom it breaks.
- `docs/adr/README.md` is the index of all ADRs.

## Consequences

- The "why" behind the pure-Rust rewrite, the transport contract, and the
  security posture becomes reviewable and durable.
- There is a small, ongoing cost to writing an ADR for each significant
  decision; trivial local choices are deliberately excluded.
- The `ROADMAP.md` swarm scheme and this ADR log together give agents a
  constitution (`VISION.md`), a plan (`ROADMAP.md`), and a rationale trail
  (`docs/adr/`) to work against.
</content>
