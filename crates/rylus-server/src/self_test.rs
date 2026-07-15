//! Built-in self-test for `rylus-server --self-test`.
//!
//! Exercises the maximum robustly-headless subset of the boot → capture →
//! encode → WebSocket accept → clean-exit chain:
//!
//! - **Capture** uses [`rylus_capture::testsrc::TestCapturable`], a synthetic
//!   pixel source that requires no real display, X11 connection, or PipeWire
//!   session.  The full capture API (recorder construction + `capture()`) is
//!   exercised; only the backend is synthetic.
//! - **Encode** passes the captured frame through `VideoEncoder` with software
//!   H.264.  The encoder is then dropped (flushed); H.264 may buffer the first
//!   frame until flush time, so zero bytes before drop is expected and OK.
//! - **WebSocket** boots the real Rylus HTTP/WebSocket server on 127.0.0.1 on
//!   an OS-assigned ephemeral port, connects via raw TCP, performs an HTTP/1.1
//!   Upgrade handshake, and verifies a `101 Switching Protocols` response.
//!
//! Exit codes: 0 = PASS, 1 = FAIL (non-zero on any step failure or timeout).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser as _;
use tracing::info;

use rylus_capture::testsrc::{PixelFormat, TestCapturable};
use rylus_core::config::Config;
use rylus_core::Capturable;
use rylus_encode::{EncoderOptions, VideoEncoder};

use crate::rylus::Rylus;

/// Hard timeout: if any step hangs, the watcher thread kills the process.
const SELF_TEST_TIMEOUT_SECS: u64 = 30;

/// Find a free loopback port by binding to 0, recording the OS-assigned port,
/// then dropping the listener.  The tiny race window between drop and re-bind
/// is acceptable in CI practice.
fn find_free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("failed to bind for port discovery: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("failed to get local addr: {e}"))?
        .port();
    Ok(port) // listener drops here, freeing the port
}

/// Step 1: capture one frame via the synthetic TestCapturable.
/// No real display, X11 session, or PipeWire daemon is required.
fn step_capture() -> Result<(usize, usize), String> {
    info!("[self-test] step 1: capture (synthetic source)");
    let capturable = TestCapturable {
        width: 320,
        height: 240,
        pixel_format: PixelFormat::BGR0,
    };
    let mut recorder = capturable
        .recorder(false)
        .map_err(|e| format!("recorder init failed: {e}"))?;
    let pixel_data = recorder
        .capture()
        .map_err(|e| format!("capture() failed: {e}"))?;
    let (w, h) = pixel_data.size();
    info!("[self-test] step 1: captured {w}×{h} frame OK");
    Ok((w, h))
}

/// Step 2: encode one synthetic frame with software H.264.
/// Hardware acceleration paths (VAAPI, NVENC, VideoToolbox, MediaFoundation)
/// are not exercised — software fallback is always available in CI.
fn step_encode(width: usize, height: usize) -> Result<(), String> {
    info!("[self-test] step 2: encode ({width}×{height}, software H.264)");

    // Re-create the capturable inside this scope so the PixelProvider lifetime
    // is tied to a local recorder — no cross-scope borrow.
    let capturable = TestCapturable {
        width,
        height,
        pixel_format: PixelFormat::BGR0,
    };
    let mut recorder = capturable
        .recorder(false)
        .map_err(|e| format!("recorder init for encode step: {e}"))?;
    let pixel_data = recorder
        .capture()
        .map_err(|e| format!("capture() for encode step: {e}"))?;

    let bytes_out = Arc::new(Mutex::new(0usize));
    let counter = bytes_out.clone();
    let mut encoder = VideoEncoder::new(
        width,
        height,
        width,
        height,
        move |data| {
            *counter.lock().unwrap() += data.len();
        },
        EncoderOptions::default(),
    )
    .map_err(|e| format!("VideoEncoder::new failed: {e}"))?;

    // encode() borrows pixel_data (and thus recorder) for the duration of the
    // call; recorder is still alive here.
    encoder.encode(pixel_data);

    // Drop to flush: H.264 may hold the first frame until end of GOP; bytes
    // written may be 0 before flush.  The important thing is that neither
    // VideoEncoder::new nor encode() panicked.
    drop(encoder);

    let total = *bytes_out.lock().unwrap();
    info!("[self-test] step 2: encode+flush OK ({total} bytes produced)");
    Ok(())
}

/// Step 3: boot the Rylus server on a loopback port, perform a WebSocket
/// upgrade handshake, verify `101 Switching Protocols`, then stop.
///
/// TLS is disabled for the self-test to avoid certificate generation latency
/// and to keep the raw-TCP client simple.  No access code is set so
/// authentication is skipped.  No Origin header is sent so the server's
/// origin-check passes (non-browser client path).
fn step_websocket(port: u16) -> Result<(), String> {
    info!("[self-test] step 3: WebSocket handshake on 127.0.0.1:{port}");

    let conf = Config::parse_from([
        "rylus".to_string(),
        "--no-gui".to_string(),
        "--no-mdns".to_string(),
        "--tls-mode".to_string(),
        "disabled".to_string(),
        "--bind-address".to_string(),
        "127.0.0.1".to_string(),
        "--web-port".to_string(),
        port.to_string(),
    ]);

    let mut server = Rylus::new();
    let started = server.start(&conf, Box::new(|_| {}));
    if !started {
        return Err("Rylus::start() returned false".into());
    }
    info!("[self-test] step 3: server bound and listening");

    // Raw HTTP/1.1 WebSocket upgrade request.  The server's ws_origin_matches_host
    // check returns true when no Origin header is present (non-browser client
    // path), so no Origin is included here.
    let request = format!(
        "GET /ws HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n"
    );

    let mut stream =
        TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("set_read_timeout: {e}"))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write request: {e}"))?;

    // Read until the end of the HTTP response headers (\r\n\r\n).
    let mut response = String::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        response.push_str(&String::from_utf8_lossy(&buf[..n]));
        if response.contains("\r\n\r\n") {
            break;
        }
    }

    drop(stream); // close the client side

    if !response.contains("101") {
        server.stop();
        return Err(format!(
            "expected 101 Switching Protocols, got: {:?}",
            response.lines().next().unwrap_or("(empty response)")
        ));
    }

    // RFC 6455 §4.2.2: the response MUST carry Sec-WebSocket-Accept derived
    // from the request key, plus Upgrade/Connection headers. Browsers fail the
    // connection without them, so a bare 101 is not a working handshake.
    // "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=" is the RFC's own sample accept value for
    // the sample nonce sent above.
    let response_lower = response.to_lowercase();
    if !response.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=") {
        server.stop();
        return Err(format!(
            "response missing/incorrect Sec-WebSocket-Accept header (browsers reject this): {response:?}"
        ));
    }
    if !response_lower.contains("upgrade: websocket") {
        server.stop();
        return Err("response missing 'Upgrade: websocket' header".into());
    }

    info!("[self-test] step 3: got 101 Switching Protocols with valid accept key OK");
    server.stop();
    Ok(())
}

fn run_self_test() -> Result<(), String> {
    let (w, h) = step_capture()?;
    step_encode(w, h)?;
    let port = find_free_port()?;
    step_websocket(port)?;
    Ok(())
}

/// Entry point called from `main` when `--self-test` is passed.
///
/// Prints a single `PASS` or `FAIL` line to stdout/stderr and exits with
/// code 0 or 1 respectively.  A background watchdog thread terminates the
/// process with exit code 1 if any step exceeds [`SELF_TEST_TIMEOUT_SECS`].
pub fn run() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(SELF_TEST_TIMEOUT_SECS));
        eprintln!(
            "FAIL: rylus-server --self-test timed out after {SELF_TEST_TIMEOUT_SECS}s"
        );
        std::process::exit(1);
    });

    match run_self_test() {
        Ok(()) => {
            println!("PASS: rylus-server --self-test");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("FAIL: rylus-server --self-test: {e}");
            std::process::exit(1);
        }
    }
}
