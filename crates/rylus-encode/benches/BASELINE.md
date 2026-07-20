# encode benchmark baseline

First baseline snapshot for `crates/rylus-encode/benches/encode.rs`
(`M1.P3.S1.T1`). Later ticks should compare against this rather than
re-deriving a number from scratch.

## Host

```
$ uname -a
Linux archlinux 7.1.3-zen2-2-zen #1 ZEN SMP PREEMPT_DYNAMIC Thu, 16 Jul 2026 17:41:12 +0000 x86_64 GNU/Linux

CPU: 12th Gen Intel(R) Core(TM) i9-12900K (24 logical cores)
FFmpeg: n8.1.2
```

No GPU encode hardware exposed to Cargo on this box — bench runs entirely
through the `libx264` software fallback path (`EncoderOptions::default()`,
`try_vaapi`/`try_nvenc`/etc. all `false`).

## Run command

```
cargo bench -p rylus-encode --bench encode -- --sample-size 20 --measurement-time 5
```

(Short sample-size/measurement-time used to keep this baseline run under a
couple of minutes; not a full statistical criterion run.)

## Result

Benchmark: `encode_1280x720_bgr0` — single `VideoEncoder::encode()` call per
iteration, 1280x720 in/out, BGR0 synthetic gradient frame, libx264
`ultrafast`/`zerolatency`/`crf=23`, `all_intra = false`.

```
encode_1280x720_bgr0    time:   [660.96 µs 667.94 µs 674.14 µs]
```

~668 µs/frame measured (mean), i.e. headroom for well over 1000 fps of
1280x720 software encode on this CPU — expected, since most frames in the
default GOP=12 P-frame stream are cheap (skip-heavy `ultrafast` P-frames);
periodic I-frames cost substantially more (libx264's own end-of-run stats
report an average size of ~88.7 KB for I-frames vs ~2.7 KB for P-frames over
this run).

Note: FFmpeg logs `Application provided invalid, non monotonically
increasing dts to muxer` warnings during the run. This is expected artifact
of the benchmark driving `encode()` far faster than real time (PTS is
derived from wall-clock `Instant`, so back-to-back synthetic iterations can
land in the same millisecond); it does not indicate a benchmark or encoder
correctness problem.
