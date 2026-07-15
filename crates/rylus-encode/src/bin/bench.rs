/// Latency budget benchmark harness for rylus.
///
/// Targets:
///   1. Encoder construction — `VideoEncoder::new()` alone (codec probe, filter
///      graph construction) via libx264, display-free. Paid once per session.
///   2. Steady-state per-frame encode — one `VideoEncoder`, encode N frames of
///      synthetic BGR0 via libx264, median over frames [2, N) so first-GOP /
///      pipeline warm-up doesn't skew the number that actually matters for the
///      product's stylus-latency budget: steady-state per-frame cost.
///   3. Protocol message serialization — serde_json serialize of representative
///      MessageOutbound variants (HeartbeatAck, CapturableList, VideoInit, Hello).
///   4. Protocol message deserialization — serde_json deserialize of the same messages.
///
/// (1) and (2) used to be a single "encode_frame" target that timed
/// `VideoEncoder::new()` + one `encode()` call together. That conflated
/// one-time construction cost with steady-state per-frame cost — in
/// production the encoder is built once per session and reused for
/// thousands of frames, so a regression in construction (e.g. codec probing)
/// could trip, or mask, the same budget as a regression in per-frame encode
/// time. Splitting them makes each budget mean what its name says.
///
/// All targets are deterministic CPU paths with no hardware, display, or network
/// dependency. Input events (rylus-input) are omitted: the device trait requires
/// a live Capturable and dispatches to kernel/hardware; no clean deterministic
/// encode entry point exists.
///
/// Budget ceilings are set at ~3× the observed median on the self-hosted
/// Arch Linux CI runner (see each BUDGET_* constant for derivation notes).
/// The binary exits non-zero if any target exceeds its ceiling.
use std::time::Instant;

use rylus_core::pixel::PixelProvider;
use rylus_core::protocol::{
    Button, Hello, MessageInbound, MessageOutbound, PointerEvent, PointerEventType, PointerType,
    WheelEvent, PROTOCOL_VERSION,
};
use rylus_encode::{EncoderOptions, VideoEncoder};

// ============================================================
// Budget ceilings (µs)
//
// Derivation: run the bench in release mode, observe median, multiply by 3
// (see each constant below for its specific derivation).
// ============================================================

mod budgets {
    /// Maximum allowed median latency (µs) for `VideoEncoder::new()` alone
    /// (codec probe + filter graph construction for libx264, 64x64). Paid
    /// once per session, not per frame.
    /// Observed median (dev workstation, release build): ~2 900 µs. Budget =
    /// 3× ~= 8 700, rounded up to 15 000 for headroom against the (slower,
    /// self-hosted) CI runner and to give this newly-split metric a first
    /// CI run before tightening.
    pub const ENCODER_INIT_US: u64 = 15_000;

    /// Maximum allowed median latency (µs) for one steady-state software
    /// H.264 encode call (`encode()` on an already-constructed 64x64
    /// libx264 ultrafast/zerolatency encoder, median of frames [2, N)).
    /// Observed median (dev workstation, release build): ~35 µs. Budget
    /// rounded up generously to 2 000 for headroom against the CI runner and
    /// to give this newly-split metric a first CI run before tightening.
    pub const ENCODE_FRAME_US: u64 = 2_000;

    /// Maximum allowed median latency (µs) for serializing one representative
    /// outbound protocol message to JSON (serde_json::to_string).
    /// Observed median on CI: <5 µs. Budget = 3× = 15, rounded to 100.
    pub const PROTO_SER_US: u64 = 100;

    /// Maximum allowed median latency (µs) for deserializing one inbound protocol
    /// message from JSON (serde_json::from_str).
    /// Observed median on CI: <5 µs. Budget = 3× = 15, rounded to 100.
    pub const PROTO_DE_US: u64 = 100;
}

// ============================================================
// Helpers
// ============================================================

fn median_us(mut samples: Vec<u64>) -> u64 {
    samples.sort_unstable();
    let n = samples.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        samples[n / 2]
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) / 2
    }
}

fn mean_us(samples: &[u64]) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    samples.iter().sum::<u64>() / samples.len() as u64
}

/// Print a bench result and return whether it passed the budget.
fn report(name: &str, median: u64, mean: u64, budget: u64) -> bool {
    let pass = median <= budget;
    let status = if pass { "PASS" } else { "FAIL" };
    println!(
        "bench::{name:30}  median={median:7} µs  mean={mean:7} µs  budget={budget:7} µs  [{status}]"
    );
    pass
}

// ============================================================
// Benchmark: software H.264 encode (libx264, display-free)
// ============================================================

/// Generate a synthetic BGR0 frame of the given dimensions.
fn make_bgr0_frame(width: usize, height: usize) -> Vec<u8> {
    let mut buf = vec![0u8; width * height * 4];
    for pixel in buf.chunks_exact_mut(4) {
        pixel[0] = 64; // B
        pixel[1] = 128; // G
        pixel[2] = 32; // R
        pixel[3] = 255; // padding
    }
    buf
}

/// Time `VideoEncoder::new()` alone — the one-time-per-session construction
/// cost (codec probe, filter graph setup). Excludes any `encode()` call.
fn bench_encoder_init(warmup: usize, iters: usize) -> Vec<u64> {
    let options = EncoderOptions::default(); // software libx264, no HW

    // Warmup: discard timings
    for _ in 0..warmup {
        let output = std::sync::Mutex::new(Vec::<u8>::new());
        let _ = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                output.lock().unwrap().extend_from_slice(data);
            },
            options,
        );
    }

    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let output = std::sync::Mutex::new(Vec::<u8>::new());
        let t0 = Instant::now();
        let enc = VideoEncoder::new(
            64,
            64,
            64,
            64,
            move |data| {
                output.lock().unwrap().extend_from_slice(data);
            },
            options,
        );
        samples.push(t0.elapsed().as_micros() as u64);
        drop(enc);
    }
    samples
}

/// Time steady-state per-frame `encode()` cost on a single, already-
/// constructed encoder — what actually matters in production, where the
/// encoder is built once per session and reused for thousands of frames.
///
/// Encodes `frames` frames and returns the elapsed time of frames `[2,
/// frames)`; the first two are discarded so first-GOP / pipeline warm-up
/// doesn't skew the steady-state number. Requires `frames >= 30` so the
/// discarded warm-up frames are a small fraction of the sample.
fn bench_encode_frame(frames: usize) -> Vec<u64> {
    assert!(
        frames >= 30,
        "bench_encode_frame needs frames >= 30 to isolate steady state"
    );
    let frame = make_bgr0_frame(64, 64);
    let options = EncoderOptions::default(); // software libx264, no HW
    let output = std::sync::Mutex::new(Vec::<u8>::new());
    let mut enc = VideoEncoder::new(
        64,
        64,
        64,
        64,
        move |data| {
            output.lock().unwrap().extend_from_slice(data);
        },
        options,
    )
    .expect("encoder should init for steady-state encode bench");

    let mut samples = Vec::with_capacity(frames - 2);
    for i in 0..frames {
        let t0 = Instant::now();
        enc.encode(PixelProvider::BGR0(64, 64, &frame));
        let elapsed = t0.elapsed().as_micros() as u64;
        if i >= 2 {
            samples.push(elapsed);
        }
    }
    samples
}

// ============================================================
// Benchmark: protocol message serialization
// ============================================================

fn make_outbound_messages() -> Vec<MessageOutbound> {
    vec![
        MessageOutbound::Hello(Hello {
            protocol_version: PROTOCOL_VERSION,
        }),
        MessageOutbound::HeartbeatAck {
            server_ts_ms: 1_700_000_000_000,
        },
        MessageOutbound::CapturableList(vec![
            "Display :0".to_string(),
            "Firefox — rylus".to_string(),
        ]),
        MessageOutbound::VideoInit {
            codec_string: "avc1.4D0028".to_string(),
        },
        MessageOutbound::ConfigOk,
        MessageOutbound::Error("connection refused".to_string()),
    ]
}

fn make_inbound_json_messages() -> Vec<String> {
    // Serialize representative inbound messages to JSON strings for the de bench.
    let messages: Vec<MessageInbound> = vec![
        MessageInbound::Heartbeat,
        MessageInbound::GetCapturableList,
        MessageInbound::Hello(Hello {
            protocol_version: PROTOCOL_VERSION,
        }),
        MessageInbound::WheelEvent(WheelEvent {
            dx: -3,
            dy: 120,
            timestamp: 999,
        }),
        MessageInbound::PointerEvent(PointerEvent {
            event_type: PointerEventType::MOVE,
            pointer_id: 1,
            timestamp: 12345,
            is_primary: true,
            pointer_type: PointerType::Pen,
            button: Button::PRIMARY,
            buttons: Button::PRIMARY,
            x: 0.5,
            y: 0.5,
            pressure: 0.8,
            tilt_x: 30,
            tilt_y: -15,
            twist: 0,
            width: 0.01,
            height: 0.01,
            altitude_angle: Some(0.785),
            azimuth_angle: Some(1.57),
        }),
    ];
    messages
        .iter()
        .map(|m| serde_json::to_string(m).expect("inbound message should serialize"))
        .collect()
}

fn bench_proto_ser(warmup: usize, iters: usize) -> Vec<u64> {
    let messages = make_outbound_messages();

    // Warmup
    for _ in 0..warmup {
        for m in &messages {
            let _ = serde_json::to_string(m).unwrap();
        }
    }

    // Measured: time the full set of messages per iteration, divide by count
    let n = messages.len() as u64;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        for m in &messages {
            let _ = serde_json::to_string(m).unwrap();
        }
        let elapsed = t0.elapsed().as_micros() as u64;
        samples.push(elapsed / n); // per-message median contribution
    }
    samples
}

fn bench_proto_de(warmup: usize, iters: usize) -> Vec<u64> {
    let json_messages = make_inbound_json_messages();

    // Warmup
    for _ in 0..warmup {
        for s in &json_messages {
            let _: MessageInbound = serde_json::from_str(s).unwrap();
        }
    }

    let n = json_messages.len() as u64;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        for s in &json_messages {
            let _: MessageInbound = serde_json::from_str(s).unwrap();
        }
        let elapsed = t0.elapsed().as_micros() as u64;
        samples.push(elapsed / n);
    }
    samples
}

// ============================================================
// Entry point
// ============================================================

fn main() {
    rylus_encode::init_ffmpeg_logger();

    println!("rylus latency budget bench");
    println!("==========================");

    const WARMUP: usize = 3;
    const ITERS: usize = 20;
    const ENCODE_FRAMES: usize = 32; // >= 30; frames [2, 32) are the measured sample

    let init_samples = bench_encoder_init(WARMUP, ITERS);
    let encode_samples = bench_encode_frame(ENCODE_FRAMES);
    let proto_ser_samples = bench_proto_ser(WARMUP, 200);
    let proto_de_samples = bench_proto_de(WARMUP, 200);

    println!();
    let mut all_pass = true;

    let init_med = median_us(init_samples.clone());
    let init_mean = mean_us(&init_samples);
    if !report("encoder_init", init_med, init_mean, budgets::ENCODER_INIT_US) {
        all_pass = false;
    }

    let enc_med = median_us(encode_samples.clone());
    let enc_mean = mean_us(&encode_samples);
    if !report("encode_frame", enc_med, enc_mean, budgets::ENCODE_FRAME_US) {
        all_pass = false;
    }

    let ser_med = median_us(proto_ser_samples.clone());
    let ser_mean = mean_us(&proto_ser_samples);
    if !report(
        "proto_message_ser",
        ser_med,
        ser_mean,
        budgets::PROTO_SER_US,
    ) {
        all_pass = false;
    }

    let de_med = median_us(proto_de_samples.clone());
    let de_mean = mean_us(&proto_de_samples);
    if !report("proto_message_de", de_med, de_mean, budgets::PROTO_DE_US) {
        all_pass = false;
    }

    println!();
    if all_pass {
        println!("result: ALL TARGETS WITHIN BUDGET");
        std::process::exit(0);
    } else {
        println!("result: BUDGET EXCEEDED — regression gate fired");
        std::process::exit(1);
    }
}
