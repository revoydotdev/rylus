//! Headless routine behind the `--self-test` CLI flag.
//!
//! Proves, without a display, GUI, or real capture hardware, that the core
//! pipeline actually works end to end:
//!
//! 1. Construct a synthetic `testsrc` capturable/recorder.
//! 2. Encode a full GOP of frames through the real software (libx264) encoder
//!    and confirm it emits packets.
//! 3. Boot a real [`Rylus`] server on a loopback port with TLS, access codes,
//!    and mDNS disabled.
//! 4. Complete a WebSocket upgrade against `/ws` from a plain TCP client to
//!    prove the server actually accepted a connection, not just bound a
//!    socket.
//!
//! Every stage is bounded so this can never hang: encoding is a fixed number
//! of synchronous frames, and the WS probe uses connect/read/write timeouts.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing::{error, info};

use rylus_capture::testsrc::{PixelFormat, TestCapturable};
use rylus_capture::Capturable;
use rylus_core::config::Config;
use rylus_core::Web2UiMessage;
use rylus_encode::{EncoderOptions, VideoEncoder};

use crate::rylus::Rylus;

const SELF_TEST_WIDTH: usize = 640;
const SELF_TEST_HEIGHT: usize = 360;
/// One full GOP. Kept in sync with the encoder's default `GOP_SIZE` so the
/// self-test genuinely exercises a keyframe followed by inter frames rather
/// than a single frame.
const SELF_TEST_GOP_FRAMES: usize = 12;
/// Bound on every network operation in the WS probe (connect, read, write).
/// The self-test must never hang, so this is deliberately short.
const SELF_TEST_NET_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs the full headless self-test. Returns `true` if every stage passed.
/// Logs a `tracing::error!` naming the failing stage on any failure.
pub fn run() -> bool {
    info!("self-test: starting headless routine (capture -> encode -> bind -> accept)");

    let packets_emitted = match run_capture_and_encode() {
        Ok(packets) => packets,
        Err(err) => {
            error!("self-test: capture/encode stage failed: {err}");
            return false;
        }
    };
    info!(
        packets_emitted,
        gop_frames = SELF_TEST_GOP_FRAMES,
        "self-test: encoder emitted packets for a full GOP"
    );

    if let Err(err) = run_bind_and_accept() {
        error!("self-test: bind/accept stage failed: {err}");
        return false;
    }

    info!("self-test: all stages passed");
    true
}

/// Stages 2-3: open a synthetic `testsrc` capturable and push a full GOP of
/// frames through the real [`VideoEncoder`]. Returns the number of packets
/// the encoder emitted while encoding those frames (not counting the fMP4
/// header fragment written at construction time).
fn run_capture_and_encode() -> Result<usize, String> {
    let capturable = TestCapturable {
        width: SELF_TEST_WIDTH,
        height: SELF_TEST_HEIGHT,
        pixel_format: PixelFormat::BGR0,
    };
    let mut recorder = capturable
        .recorder(false)
        .map_err(|err| format!("failed to open test recorder: {err}"))?;

    let packet_count = Arc::new(AtomicUsize::new(0));
    let packet_count_cb = packet_count.clone();
    let mut encoder = VideoEncoder::new(
        SELF_TEST_WIDTH,
        SELF_TEST_HEIGHT,
        SELF_TEST_WIDTH,
        SELF_TEST_HEIGHT,
        move |_data| {
            packet_count_cb.fetch_add(1, Ordering::SeqCst);
        },
        EncoderOptions::default(),
    )
    .map_err(|err| format!("encoder init failed: {err}"))?;

    // `VideoEncoder::new` already writes the fragmented-MP4 header through
    // the callback, so baseline it before encoding real frames.
    let baseline = packet_count.load(Ordering::SeqCst);
    let mut first_frame_packets = 0usize;

    for i in 0..SELF_TEST_GOP_FRAMES {
        let frame = recorder
            .capture()
            .map_err(|err| format!("test recorder capture failed on frame {i}: {err}"))?;
        let before = packet_count.load(Ordering::SeqCst);
        encoder.encode(frame);
        if i == 0 {
            first_frame_packets = packet_count.load(Ordering::SeqCst) - before;
        }
    }

    let emitted = packet_count.load(Ordering::SeqCst) - baseline;
    if emitted == 0 {
        return Err(format!(
            "encoded {SELF_TEST_GOP_FRAMES} frames but the encoder emitted no packets"
        ));
    }
    if first_frame_packets > 0 {
        info!("self-test: first frame (forced keyframe) produced a packet immediately");
    }
    Ok(emitted)
}

/// Stages 4-5: boot a real [`Rylus`] server on an internal, hardcoded
/// loopback config (TLS disabled, no access code, no mDNS) and prove a
/// WebSocket client can complete the `/ws` upgrade.
fn run_bind_and_accept() -> Result<(), String> {
    let loopback: IpAddr = IpAddr::from([127, 0, 0, 1]);

    // Reserve an ephemeral loopback port by binding then releasing it. This
    // has a theoretical TOCTOU race, but is acceptable here: we rebind on the
    // same loopback address microseconds later, inside a single process.
    let port = {
        let probe = TcpListener::bind((loopback, 0))
            .map_err(|err| format!("failed to reserve an ephemeral port: {err}"))?;
        probe
            .local_addr()
            .map_err(|err| format!("failed to read reserved port: {err}"))?
            .port()
    };
    let addr = SocketAddr::new(loopback, port);

    // A self-contained internal config for the self-test's own server
    // instance. Deliberately does not reuse whatever flags the user passed
    // alongside --self-test: the self-test is a synthetic internal check,
    // not a replay of the user's exact CLI config, so hardcoding loopback +
    // disabled TLS/mDNS/access-code here is simpler and fully deterministic.
    let mut internal_conf = Config::parse_from::<_, &str>(["rylus-self-test"]);
    internal_conf.bind_address = loopback;
    internal_conf.web_port = port;
    internal_conf.access_code = None;
    internal_conf.tls_mode = Some("disabled".to_string());
    internal_conf.no_mdns = true;

    let mut server = Rylus::new();
    let started = server.start(
        &internal_conf,
        Box::new(|msg| match msg {
            Web2UiMessage::UInputInaccessible => {}
        }),
    );
    if !started {
        return Err(format!("Rylus::start failed to bind {addr}"));
    }
    info!(address = %addr, "self-test: server bound and listening");

    let upgrade_result = ws_upgrade_probe(addr, SELF_TEST_NET_TIMEOUT);
    server.stop();
    upgrade_result?;

    info!("self-test: WS upgrade accepted");
    Ok(())
}

/// Hand-rolled minimal HTTP/1.1 WebSocket upgrade request over a raw
/// `TcpStream`. The `/ws` route in `web.rs` doesn't validate
/// `Sec-WebSocket-Key`/`Accept` (the server hands the raw upgraded stream
/// straight to tungstenite in `Role::Server` mode without negotiating a
/// handshake of its own), so a real client only needs to confirm the
/// response status line is `101 Switching Protocols` to prove the upgrade
/// was actually accepted, not merely that the socket was bound.
fn ws_upgrade_probe(addr: SocketAddr, timeout: Duration) -> Result<(), String> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|err| format!("TCP connect to {addr} failed: {err}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("failed to set read timeout: {err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("failed to set write timeout: {err}"))?;

    let request = format!(
        "GET /ws HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Connection: Upgrade\r\n\
         Upgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         \r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("failed to write upgrade request: {err}"))?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = stream
            .read(&mut chunk)
            .map_err(|err| format!("failed to read upgrade response: {err}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 8192 {
            return Err("upgrade response exceeded 8 KiB without a header terminator".to_string());
        }
    }

    let response = String::from_utf8_lossy(&buf);
    let status_line = response.lines().next().unwrap_or("");
    if status_line.contains("101") {
        Ok(())
    } else {
        Err(format!(
            "expected HTTP 101 Switching Protocols, got: {status_line:?}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_test_run_passes_and_tears_down_cleanly() {
        assert!(
            run(),
            "self-test routine should complete all stages and return true"
        );
    }
}
