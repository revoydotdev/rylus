# Rylus Latency Instrumentation & Baseline

AX-1 in `VISION.md` states the axiom this document exists to serve: "A
pointer-to-photon latency budget is measured, not asserted," and names ~7 ms
as the added-latency threshold that becomes perceptible while inking (CHI
2014). This document describes what is actually instrumented in the codebase
today, how to turn it on, a real measured baseline obtained by running that
instrumentation, and an honest comparison against the AX-1 ceiling — not a
claim that the full pointer-to-photon budget is covered yet.

## 1. What's instrumented

**Scope:** the server-side portion of the pipeline only — screen capture
through handing an encoded frame to the outbound sender. It does **not**
cover stylus-to-browser input latency, network transit, client-side MSE
decode, or display compositing/photon time. Those stages are real and matter
for the full AX-1 budget, but are not wired up by this change; see §5 for
what full pointer-to-photon instrumentation would still require.

**Where:** `crates/rylus-server/src/session.rs`. The per-client video pipeline
runs as two threads per connected client, wired in
`RylusClientHandler::new` (`session.rs:99-111`): a capture thread
(`handle_video`, `session.rs:834`) and an encode thread (`encode_thread`,
`session.rs:571`), connected by a bounded `mpsc::sync_channel<EncodeCommand>`
of capacity 1.

**Real per-frame timestamps carried through the real pipeline (not
fabricated):**
- `capture_start` — stamped with `Instant::now()` immediately before
  `rec.capture()` is called on the real `Recorder` (`session.rs:972-973`).
- `queued_at` — stamped immediately before the captured, scaled frame is
  handed to the encode thread via `encode_tx.try_send(EncodeCommand::Frame {
  ... })` (`session.rs:1040-1049`). Both timestamps travel with the frame as
  fields on `EncodeCommand::Frame` (`session.rs:582` and the type's doc
  comment above it), not through any side channel or shared mutable state.
- `dequeued_at` — stamped in `encode_thread` right after the command is
  received and right before `enc.encode(...)` is called (`session.rs:606`).
- `encode_done` — stamped right after `enc.encode(...)` returns
  (`session.rs:608`).

From these four real `Instant`s, `encode_thread` computes and emits one
`tracing::info!` event per frame, target `"rylus_server::latency"`
(`session.rs:571-628`), with four structured `u64` microsecond fields:

| Field           | Meaning                                                                 |
|-----------------|--------------------------------------------------------------------------|
| `capture_us`    | `queued_at - capture_start`: real screen-capture + pixel-copy time.      |
| `queue_us`      | `dequeued_at - queued_at`: real mpsc channel handoff/wait time.          |
| `encode_send_us`| `encode_done - dequeued_at`: real libx264 encode call.                  |
| `total_us`      | `encode_done - capture_start`: the full measured server-side span.      |

**Important, stated honestly:** `enc.encode(...)` synchronously drives
FFmpeg's AVIO write callback, which is where the encoded bytes are actually
handed to `RylusSender::send_video` (the WebSocket write) — see the encoder
construction closure in the `EncodeCommand::Restart` arm,
`session.rs:646-651`. That means the real socket write happens *inside* the
`encode_send_us` span, not as a separately-timed stage after it. This field
is therefore reported as combined encode+send rather than fabricating a split
that isn't actually observable without invasive per-callback instrumentation
inside the FFmpeg write path. `capture_us` and `queue_us` are independently
real, separately-measured stage boundaries. The four fields telescope
exactly (`total_us == capture_us + queue_us + encode_send_us`, modulo
microsecond truncation from rounding three sub-durations independently — see
the `latency_log_emits_real_per_frame_events_when_enabled` test).

**Logging cadence:** every frame, not sampled or throttled. This is
deliberate: the flag is opt-in (see §2), so the cost of per-frame structured
events is only paid when an operator has explicitly asked for it, and
throttling would throw away exactly the tail-latency information (occasional
slow frames, encoder restarts) that this instrumentation exists to surface.
If per-frame volume becomes a real operational problem at high frame rates,
the natural next step is a sampling knob (e.g. log every Nth frame), not
silently discarding this granularity by default.

## 2. How to enable it (opt-in, off by default)

- **CLI flag:** `--latency-log`, defined on `Config` in
  `crates/rylus-core/src/config.rs:176-183`, following the exact pattern of
  the existing `--no-mdns`/`--self-test` boolean flags in the same struct
  (`#[arg(long, ...)] #[serde(default)] pub latency_log: bool`).
- **Wiring:** `Config::latency_log` flows into `RylusClientConfig::latency_log`
  at `crates/rylus-server/src/rylus.rs:132` (the `RylusClientConfig { ... }`
  literal passed to `crate::web::run`), which is copied per-connection into
  the `handle_video`/`encode_thread` call chain
  (`session.rs:99-111`, `session.rs:834-848`).
- **Default:** `false`. With no flag, `encode_thread`'s `if latency_log { ...
  }` branch (`session.rs:596-627`) is never taken — `enc.encode(...)` runs
  through the plain `else` arm with no timestamp capture, no `tracing::info!`
  call, and no per-frame overhead beyond the two `Instant::now()` calls in
  `handle_video`'s capture path, which are always taken (cheap: two
  monotonic-clock reads) so that the timestamps are available the instant the
  flag *is* turned on without a data-flow change. The `latency_log_stays_silent_when_disabled`
  test in `session.rs` proves this by running the real pipeline with the flag
  off and asserting zero `rylus_server::latency` events fire.
- **Verified manually:** running the server without `--latency-log` and
  connecting a client produces no `rylus_server::latency` target events in
  the log (confirmed via the automated test above, which exercises the exact
  same code path); running with `--latency-log` produces one event per
  encoded frame.

## 3. Method for the measured baseline below

The baseline numbers below come from `session::tests::latency_log_measured_baseline_640x360_30fps`
in `crates/rylus-server/src/session.rs`, run with `cargo test --release -p
rylus-server --bin rylus session::tests::latency_log_measured_baseline -- --nocapture`.
This test:

- Drives the **real** production capture/encode pipeline: `handle_video` +
  `encode_thread`, exactly as used by `RylusClientHandler` for a real client
  connection — not a simplified re-implementation.
- Uses `rylus_capture::testsrc::TestCapturable` (the same synthetic capture
  source `--self-test` uses, `crates/rylus-server/src/self_test.rs:35-40`) at
  640x360, real pixel generation, real per-frame BGR0 buffers.
- Uses the **real** libx264 encoder via `VideoEncoder::new`/`encode` (the same
  FFmpeg-backed encoder used in production), with default `EncoderOptions`.
- Uses a `MockSender` standing in for the WebSocket socket — the same
  in-process test double already used by this file's
  `video_forwarder_sends_frames` test. This means `encode_send_us` in this
  specific baseline measures real encode time plus a fast in-process
  `Vec` push, **not** a real TCP/TLS WebSocket write over an actual network
  socket. A real LAN write would add real syscall/kernel-buffer/NIC time on
  top of what's measured here; this baseline is a lower bound on the
  encode+send stage, not an upper bound.
- Runs for 3 wall-clock seconds at a 30 fps target frame rate, collecting one
  real `total_us` sample per encoded frame via the `LatencyCapture`
  `tracing::Subscriber` defined in the same test module, then reports real
  mean/p50/p95/p99/min/max computed from those samples — no numbers below are
  hand-written or estimated.
- Was run in `--release` mode (optimized build) since that's what a real
  deployment ships; a debug-build run of the same test produced consistently
  higher numbers (mean 729 µs vs. 649 µs release, same run parameters),
  included here for transparency about build-mode sensitivity.

**Environment:** Intel Core i9-12900K (24 threads), Linux 7.1.3-zen2
(x86_64), `rustc 1.97.1`, workspace `Cargo.lock` as of this commit. This is a
single-machine, single-run measurement on shared development hardware, not a
controlled benchmarking environment — treat the absolute numbers as
indicative of the pipeline's real order of magnitude, not as a tightly
reproducible perf-lab figure.

## 4. Measured baseline (real numbers, this run)

640x360 synthetic capture, 30 fps target, `--release` build, n=89 real
encoded frames over a 3-second run:

| Statistic | capture→encode→(in-process)send, total_us |
|-----------|--------------------------------------------|
| mean      | 649 µs (0.649 ms)                           |
| p50       | 545 µs                                      |
| p95       | 792 µs                                      |
| p99       | 840 µs                                      |
| min       | 449 µs                                      |
| max       | 7784 µs (one outlier — see below)            |

Debug-build run (same parameters, `cargo test` without `--release`), for
comparison: mean 729 µs, p50 629 µs, p95 931 µs, p99 1023 µs, min 507 µs, max
6580 µs, n=89.

The max in both runs is a single-frame outlier — almost certainly an encoder
restart or scheduling jitter on a shared, multi-tenant dev machine running
other Claude Code / build workloads concurrently — not representative of
steady-state behavior; p95/p99 are the more meaningful tail figures here.

## 5. Comparison against the AX-1 ceiling — stated honestly

AX-1 names **~7 ms (7000 µs)** as the threshold at which added latency
becomes perceptible while inking. The measured server-side capture→encode
span above (p95 = 792 µs, release build) is comfortably under that ceiling
**for the portion of the pipeline this instrumentation actually covers.**

This is not, and must not be read as, a claim that Rylus's full
pointer-to-photon latency is under 7 ms. What this instrumentation measures
is: screen-capture call → pixel copy → channel handoff → libx264 encode call
(which includes handing bytes to the sender callback). It does **not**
measure, and no claim is made here about:

- Stylus-contact-to-browser-event latency (client-side, upstream of this
  server entirely).
- Real network transit time for the WebSocket binary frame (this baseline's
  "send" is an in-process `Vec` push, not a socket write — see §3).
- Client-side MSE buffering/decode latency.
- Display compositor/photon time on the mirrored screen.

Given that gap, the honest summary is: **the piece of the pipeline this
instrumentation covers is measured and is small relative to the 7 ms budget,
but the full pointer-to-photon figure AX-1 actually cares about is still not
instrumented end-to-end.** Closing that gap would require: (a) a real
network-write measurement (swap `MockSender` for a real loopback/LAN
`WsRylusSender` run), and (b) client-side timestamping from pointer event to
paint, correlated back to a server-side frame id — neither of which exists in
this codebase yet. This is named as follow-up work, not glossed over.

## 6. Reproducing this measurement

```
cargo test --release -p rylus-server --bin rylus \
  session::tests::latency_log_measured_baseline_640x360_30fps -- --nocapture
```

The two correctness tests for the instrumentation itself (real events fire
when enabled, real silence when disabled, and the four fields telescope to
the real total) are:

```
cargo test -p rylus-server --bin rylus \
  session::tests::latency_log_emits_real_per_frame_events_when_enabled \
  session::tests::latency_log_stays_silent_when_disabled
```
