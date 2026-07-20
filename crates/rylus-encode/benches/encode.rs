//! Criterion benchmark for the `VideoEncoder::encode` hot path.
//!
//! Drives a single software (libx264) encoder instance with a fixed synthetic
//! BGR0 frame buffer, so it runs on any machine — no capture hardware or
//! display required.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use rylus_core::pixel::PixelProvider;
use rylus_encode::{EncoderOptions, VideoEncoder};

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

/// Build a fixed synthetic BGR0 frame (a horizontal gradient) of the given
/// dimensions. Doesn't need to resemble a real screen capture — just needs to
/// be correctly sized so `encode()` doesn't bail out early.
fn make_bgr0_frame(width: usize, height: usize) -> Vec<u8> {
    let mut buf = vec![0u8; width * height * 4];
    for (i, pixel) in buf.chunks_exact_mut(4).enumerate() {
        let x = (i % width) as u8;
        pixel[0] = x; // B
        pixel[1] = x.wrapping_mul(3); // G
        pixel[2] = x.wrapping_mul(7); // R
        pixel[3] = 255; // padding
    }
    buf
}

fn encode_benchmark(c: &mut Criterion) {
    let frame = make_bgr0_frame(WIDTH, HEIGHT);

    // Software encode only (no VAAPI/NVENC/VideoToolbox/MediaFoundation) so
    // this runs identically on any dev box.
    let mut encoder = VideoEncoder::new(
        WIDTH,
        HEIGHT,
        WIDTH,
        HEIGHT,
        |_data| {},
        EncoderOptions::default(),
    )
    .expect("encoder should initialize with libx264 software fallback");

    c.bench_function("encode_1280x720_bgr0", |b| {
        b.iter(|| {
            encoder.encode(PixelProvider::BGR0(
                WIDTH,
                HEIGHT,
                black_box(frame.as_slice()),
            ));
        });
    });
}

criterion_group!(benches, encode_benchmark);
criterion_main!(benches);
