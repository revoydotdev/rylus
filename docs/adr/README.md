# Architecture Decision Records

This directory records the load-bearing architectural decisions behind Rylus,
following the lightweight format defined in
[ADR-0001](ADR-0001-record-architecture-decisions.md). Each ADR captures one
decision (Context / Decision / Consequences) and, once **Accepted**, is
immutable — a changed decision is captured by a new ADR that supersedes the old
one. Every ADR must be consistent with [`../../VISION.md`](../../VISION.md) or
explicitly supersede the axiom it breaks.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](ADR-0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](ADR-0002-pure-rust-modular-workspace.md) | Pure-Rust modular workspace | Accepted |
| [0003](ADR-0003-lan-only-websocket-transport-v1.md) | LAN-only, WebSocket-only transport for v1 | Accepted |
| [0004](ADR-0004-secure-by-default-network-posture.md) | Secure-by-default posture for a network-exposed service | Accepted |

## Conventions

- Filename: `ADR-NNNN-short-title.md`, zero-padded sequential number.
- Status: `Proposed` → `Accepted` → (optionally) `Superseded by ADR-NNNN`.
- Add a row to the table above when you add an ADR.
</content>
