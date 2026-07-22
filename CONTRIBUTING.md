# Contributing to Rylus

Thanks for taking an interest. Rylus is a small project; the bar for contributions is "does it build, test, lint, and explain itself." This document covers everything you need to do that.

## Code of conduct

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md). Please read it before opening issues or pull requests.

## Repo shape

Rylus is a Cargo workspace plus a TypeScript browser client:

```
crates/
  rylus-core        # Config, protocol, shared traits and types
  rylus-capture     # Screen capture backends (X11, PipeWire, CoreGraphics, GDI)
  rylus-encode      # H.264 / fMP4 via FFmpeg
  rylus-input       # Input synthesis (uinput, WinRT, enigo)
  rylus-transport   # WebSocket transport
  rylus-gui         # egui desktop GUI
  rylus-server      # HTTP / WebSocket server, main binary
ts/                 # TypeScript sources for the tablet client (built with esbuild)
www/                # Static assets and HTML templates
packaging/          # AUR PKGBUILDs, systemd unit, icons, .desktop
.github/workflows/  # CI
```

See [DESIGN.md](DESIGN.md) for the visual / UX design system and [ROADMAP.md](ROADMAP.md) for the path to 1.0.

## Prerequisites

- **Rust** — toolchain matching the `rust-version` declared in [`Cargo.toml`](Cargo.toml). `rustup toolchain install stable` is enough.
- **Node 22+** — for the TypeScript client and tests.
- **FFmpeg** dev headers — `libavcodec`, `libavformat`, `libavutil`, `libswscale`, `libswresample`.
- **Linux only**: `pkg-config`, `pipewire`, `xdg-desktop-portal`, X11 dev headers (for the X11 backend).

The repo's `build_in_local_container.sh` and `docker_build.sh` document a known-good Linux build environment.

## Build, test, lint

```sh
# Rust workspace
cargo check  --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test   --workspace
cargo fmt    --all -- --check

# TypeScript client
npm ci
npm run build      # produces www/static/lib.js, www/static/sw.js
npm test           # vitest
```

CI runs all of the above on every PR. Treat clippy `-D warnings` and `cargo fmt --check` as required.

## Commit conventions

We follow [Conventional Commits](https://www.conventionalcommits.org/). Prefixes used in this repo:

- `feat:` — new user-visible behaviour
- `fix:` — bug fix
- `perf:` — performance improvement, no behaviour change
- `refactor:` — internal change with no behaviour change
- `docs:` — docs only
- `test:` — tests only
- `chore:` / `ci:` — tooling, deps, packaging, CI configuration

Keep commits surgical and single-concern; prefer rebase over merge for clean history. Don't include AI-tool attribution lines (`Co-Authored-By: Claude ...`, `Generated with ...`); a commit message is enough.

## Pull requests

1. Open an issue first for anything non-trivial, so we can sanity-check the approach before you spend time on it.
2. Branch from `master`; keep PRs focused (one feature, one fix, one refactor).
3. Make sure the test, clippy, and fmt commands above are clean locally before pushing.
4. Update [`CHANGELOG.md`](CHANGELOG.md) under "Unreleased" if your change is user-visible.
5. If you're adding or modifying `unsafe` code, include a `// SAFETY:` comment describing the invariants the caller relies on.

PRs are reviewed for correctness, scope, and style. Maintainers may push small fixups directly onto your branch — let us know in the PR if you'd rather we didn't.

## Areas that need help

- Wayland edge cases — compositor-specific quirks, cursor capture, per-window mapping
- Windows packaging (signed MSI via WiX)
- macOS notarized DMG
- Performance profiling on lower-power tablets (older iPads, Android via Firefox)
- Accessibility audit of the browser client

The [ROADMAP](ROADMAP.md) lists in-flight milestones with their exit criteria.

## Reporting bugs

File a [GitHub issue](https://github.com/Chorosyne/rylus/issues) with:
- Rylus version (`rylus --version`)
- OS, version, and (on Linux) compositor / X11 vs Wayland
- Tablet device and browser
- Steps to reproduce
- Relevant log output (`RYLUS_LOG_LEVEL=debug RYLUS_LOG_JSON=true rylus ...`)

## License

By submitting a contribution you agree it will be licensed under the project's [AGPL-3.0-or-later](LICENSE) license.
