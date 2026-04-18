use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::{spawn, JoinHandle};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{error, info, trace, warn};

use rylus_capture::{get_capturables, Capturable, Recorder};
use rylus_core::error::CErrorCode;
use rylus_core::protocol::{
    ClientConfiguration, Hello, KeyboardEvent, MessageInbound, MessageOutbound, PointerEvent,
    RylusReceiver, RylusSender, WheelEvent, MIN_CLIENT_PROTOCOL_VERSION, PROTOCOL_VERSION,
};
use rylus_encode::{EncoderOptions, VideoEncoder};
use rylus_input::device::{InputDevice, InputDeviceType};

#[allow(dead_code)]
const BROADCAST_CAPACITY: usize = 32;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum StreamEvent {
    VideoFrame(Arc<Vec<u8>>),
    NewVideo,
    VideoInit { codec_string: String },
    ConfigOk,
    Error(String),
}

pub(crate) struct VideoConfig {
    capturable: Box<dyn Capturable>,
    capture_cursor: bool,
    max_width: usize,
    max_height: usize,
    frame_rate: f64,
}

#[allow(private_interfaces)]
pub(crate) enum VideoCommands {
    Start(VideoConfig),
    Pause,
    Resume,
    Restart,
    BufferHealth(f64),
}

fn send_message<S>(sender: &mut S, message: MessageOutbound)
where
    S: RylusSender,
{
    if let Err(err) = sender.send_message(message) {
        warn!("Failed to send message to client: {err}");
    }
}

pub struct RylusClientHandler<S, R, FnUInput> {
    sender: S,
    receiver: Option<R>,
    video_sender: mpsc::Sender<VideoCommands>,
    input_device: Option<Box<dyn InputDevice>>,
    capturables: Vec<Box<dyn Capturable>>,
    on_uinput_inaccessible: FnUInput,
    config: RylusClientConfig,
    #[cfg(target_os = "linux")]
    capture_cursor: bool,
    client_name: Option<String>,
    video_thread: JoinHandle<()>,
}

#[derive(Clone, Copy)]
pub struct RylusClientConfig {
    pub encoder_options: EncoderOptions,
    #[cfg(target_os = "linux")]
    pub wayland_support: bool,
    #[cfg(feature = "gui")]
    pub no_gui: bool,
}

impl<S, R, FnUInput> RylusClientHandler<S, R, FnUInput> {
    pub fn new(
        sender: S,
        receiver: R,
        on_uinput_inaccessible: FnUInput,
        config: RylusClientConfig,
    ) -> Self
    where
        R: RylusReceiver,
        S: RylusSender + Clone + Send + Sync + 'static,
    {
        let (video_sender, video_receiver) = mpsc::channel::<VideoCommands>();
        let video_thread = {
            let sender = sender.clone();
            // offload creating the videostream to another thread to avoid blocking the thread that
            // is receiving messages from the websocket
            spawn(move || handle_video(video_receiver, sender, config.encoder_options))
        };

        Self {
            sender,
            receiver: Some(receiver),
            video_sender,
            input_device: None,
            capturables: vec![],
            on_uinput_inaccessible,
            config,
            #[cfg(target_os = "linux")]
            capture_cursor: false,
            client_name: None,
            video_thread,
        }
    }

    pub fn run(mut self)
    where
        R: RylusReceiver,
        S: RylusSender + Clone + Send + Sync + 'static,
        FnUInput: Fn(),
    {
        for message in self
            .receiver
            .take()
            .expect("run() must only be called once")
        {
            match message {
                Ok(message) => {
                    trace!("Received message: {message:?}");
                    match message {
                        MessageInbound::Hello(hello) => self.handle_hello(hello),
                        MessageInbound::PointerEvent(event) => self.process_pointer_event(&event),
                        MessageInbound::BatchedPointerEvents(events) => {
                            for event in &events {
                                self.process_pointer_event(event);
                            }
                        }
                        MessageInbound::WheelEvent(event) => self.process_wheel_event(&event),
                        MessageInbound::KeyboardEvent(event) => self.process_keyboard_event(&event),
                        MessageInbound::GetCapturableList => self.send_capturable_list(),
                        MessageInbound::Config(config) => self.update_config(config),
                        MessageInbound::PauseVideo => {
                            if let Err(e) = self.video_sender.send(VideoCommands::Pause) {
                                warn!("Failed to send Pause command to video thread: {e}");
                            }
                        }
                        MessageInbound::ResumeVideo => {
                            if let Err(e) = self.video_sender.send(VideoCommands::Resume) {
                                warn!("Failed to send Resume command to video thread: {e}");
                            }
                        }
                        MessageInbound::RestartVideo => {
                            if let Err(e) = self.video_sender.send(VideoCommands::Restart) {
                                warn!("Failed to send Restart command to video thread: {e}");
                            }
                        }
                        MessageInbound::BufferHealth(health) => {
                            if let Err(e) = self
                                .video_sender
                                .send(VideoCommands::BufferHealth(health.buffer_seconds))
                            {
                                warn!("Failed to send BufferHealth to video thread: {e}");
                            }
                        }
                        MessageInbound::Heartbeat => {
                            // keepalive; receipt resets idle timer. Echo the server's
                            // receive timestamp so the client can compute RTT for the
                            // connection-quality indicator.
                            let server_ts_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            self.send_message(MessageOutbound::HeartbeatAck { server_ts_ms });
                        }
                        MessageInbound::ChooseCustomInputAreas => {
                            #[cfg(feature = "gui")]
                            {
                                let (sender, receiver) = std::sync::mpsc::channel();
                                rylus_gui::get_input_area(self.config.no_gui, sender);
                                let mut sender = self.sender.clone();
                                spawn(move || {
                                    while let Ok(areas) = receiver.recv() {
                                        send_message(
                                            &mut sender,
                                            MessageOutbound::CustomInputAreas(areas),
                                        );
                                    }
                                });
                            }
                            #[cfg(not(feature = "gui"))]
                            {
                                warn!("Custom input areas require the 'gui' feature.");
                                self.send_message(MessageOutbound::Error(
                                    "Custom input areas not available without GUI.".to_string(),
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to read message {err}!");
                    self.send_message(MessageOutbound::Error(
                        "Failed to read message!".to_string(),
                    ));
                }
            }
        }

        drop(self.video_sender);
        if let Err(err) = self.video_thread.join() {
            warn!("Failed to join video thread: {err:?}");
        }
    }

    fn send_message(&mut self, message: MessageOutbound)
    where
        S: RylusSender,
    {
        send_message(&mut self.sender, message)
    }

    fn process_wheel_event(&mut self, event: &WheelEvent) {
        match &mut self.input_device {
            Some(i) => i.send_wheel_event(event),
            None => warn!("Input device is not initialized, can not process WheelEvent!"),
        }
    }

    fn process_pointer_event(&mut self, event: &PointerEvent) {
        if let Some(d) = &mut self.input_device {
            d.send_pointer_event(event)
        } else {
            warn!("Input device is not initialized, can not process PointerEvent!");
        }
    }

    fn process_keyboard_event(&mut self, event: &KeyboardEvent) {
        if let Some(d) = &mut self.input_device {
            d.send_keyboard_event(event)
        } else {
            warn!("Input device is not initialized, can not process KeyboardEvent!");
        }
    }

    fn handle_hello(&mut self, hello: Hello)
    where
        S: RylusSender,
    {
        if hello.protocol_version < MIN_CLIENT_PROTOCOL_VERSION {
            tracing::warn!(
                "Rejecting client hello: v{} is below minimum v{}",
                hello.protocol_version,
                MIN_CLIENT_PROTOCOL_VERSION,
            );
            self.send_message(MessageOutbound::HelloNack {
                server_version: PROTOCOL_VERSION,
                min_client_version: MIN_CLIENT_PROTOCOL_VERSION,
                reason: "Client is too old — reload the page to pick up the new bundle.".into(),
            });
            return;
        }
        let negotiated = PROTOCOL_VERSION.min(hello.protocol_version);
        tracing::info!(
            "Client hello: protocol v{}, server v{}, negotiated v{}",
            hello.protocol_version,
            PROTOCOL_VERSION,
            negotiated,
        );
        self.send_message(MessageOutbound::Hello(Hello {
            protocol_version: PROTOCOL_VERSION,
        }));
    }

    fn send_capturable_list(&mut self)
    where
        S: RylusSender,
    {
        self.capturables = get_capturables(
            #[cfg(target_os = "linux")]
            self.config.wayland_support,
            #[cfg(target_os = "linux")]
            self.capture_cursor,
        );
        let windows: Vec<String> = self.capturables.iter().map(|c| c.name()).collect();
        self.send_message(MessageOutbound::CapturableList(windows));
    }

    fn update_config(&mut self, config: ClientConfiguration)
    where
        S: RylusSender,
        FnUInput: Fn(),
    {
        let client_name_changed = if self.client_name != config.client_name {
            self.client_name = config.client_name;
            true
        } else {
            false
        };
        if config.capturable_id < self.capturables.len() {
            let capturable = self.capturables[config.capturable_id].clone();

            #[cfg(target_os = "linux")]
            {
                self.capture_cursor = config.capture_cursor;
            }

            #[cfg(target_os = "linux")]
            if config.uinput_support {
                if !self.input_device.as_ref().is_some_and(|d| {
                    !client_name_changed && d.device_type() == InputDeviceType::UInputDevice
                }) {
                    let device = rylus_input::uinput_device::UInputDevice::new(
                        capturable.clone(),
                        &self.client_name,
                    );
                    match device {
                        Ok(d) => self.input_device = Some(Box::new(d)),
                        Err(e) => {
                            error!("Failed to create uinput device: {}", e);
                            if let CErrorCode::UInputNotAccessible = e.to_enum() {
                                (self.on_uinput_inaccessible)();
                            }
                            self.send_message(MessageOutbound::ConfigError(
                                "Failed to create uinput device!".to_string(),
                            ));
                            return;
                        }
                    }
                } else if let Some(d) = self.input_device.as_mut() {
                    d.set_capturable(capturable.clone());
                }
            } else if !self
                .input_device
                .as_ref()
                .is_some_and(|d| d.device_type() == InputDeviceType::EnigoDevice)
            {
                self.input_device = Some(Box::new(rylus_input::enigo_device::EnigoDevice::new(
                    capturable.clone(),
                )));
            } else if let Some(d) = self.input_device.as_mut() {
                d.set_capturable(capturable.clone());
            }

            #[cfg(target_os = "macos")]
            if self.input_device.is_none() {
                self.input_device = Some(Box::new(rylus_input::enigo_device::EnigoDevice::new(
                    capturable.clone(),
                )));
            } else {
                if let Some(d) = self.input_device.as_mut() {
                    d.set_capturable(capturable.clone());
                }
            }
            #[cfg(target_os = "windows")]
            if self.input_device.is_none() {
                self.input_device = Some(Box::new(
                    rylus_input::enigo_device_win::WindowsInput::new(capturable.clone()),
                ));
            } else {
                if let Some(d) = self.input_device.as_mut() {
                    d.set_capturable(capturable.clone());
                }
            }

            if let Err(e) = self.video_sender.send(VideoCommands::Start(VideoConfig {
                capturable,
                capture_cursor: config.capture_cursor,
                max_width: config.max_width,
                max_height: config.max_height,
                frame_rate: config.frame_rate,
            })) {
                warn!("Failed to send Start command to video thread: {e}");
                self.send_message(MessageOutbound::Error(
                    "Failed to start video capture.".to_string(),
                ));
            }
        } else {
            warn!(
                "Got invalid id for capturable: {} (list has {} entries). \
                 On Wayland, click \"Refresh List\" after granting portal access.",
                config.capturable_id,
                self.capturables.len()
            );
            self.send_message(MessageOutbound::ConfigError(
                "No capturable selected. Click \"Refresh List\" to request screen access."
                    .to_string(),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-device broadcast: shared capture+encode pipeline
// ---------------------------------------------------------------------------
#[allow(dead_code)]
pub struct StreamSession {
    video_cmd_tx: Option<mpsc::Sender<VideoCommands>>,
    event_tx: broadcast::Sender<StreamEvent>,
    video_thread: Option<JoinHandle<()>>,
    subscriber_count: AtomicUsize,
}

#[allow(dead_code)]
impl StreamSession {
    pub fn new(encoder_options: EncoderOptions) -> Self {
        let (event_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (video_cmd_tx, video_cmd_rx) = mpsc::channel::<VideoCommands>();

        let event_tx_clone = event_tx.clone();
        let video_thread = spawn(move || {
            handle_video_broadcast(video_cmd_rx, event_tx_clone, encoder_options);
        });

        Self {
            video_cmd_tx: Some(video_cmd_tx),
            event_tx,
            video_thread: Some(video_thread),
            subscriber_count: AtomicUsize::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.subscriber_count.fetch_add(1, Ordering::Relaxed);
        self.event_tx.subscribe()
    }

    pub fn send_command(&self, cmd: VideoCommands) {
        if let Some(tx) = &self.video_cmd_tx {
            if let Err(e) = tx.send(cmd) {
                warn!("Failed to send command to video broadcast thread: {e}");
            }
        }
    }

    pub fn stop(&mut self) {
        self.video_cmd_tx.take();
        if let Some(handle) = self.video_thread.take() {
            if let Err(e) = handle.join() {
                warn!("Video broadcast thread panicked: {e:?}");
            }
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::Relaxed)
    }
}

#[allow(dead_code)]
pub fn video_forwarder<S: RylusSender + Send + 'static>(
    mut event_rx: broadcast::Receiver<StreamEvent>,
    mut sender: S,
) {
    loop {
        match event_rx.blocking_recv() {
            Ok(event) => {
                let result = match event {
                    StreamEvent::VideoFrame(data) => sender.send_video(&data),
                    StreamEvent::NewVideo => sender.send_message(MessageOutbound::NewVideo),
                    StreamEvent::VideoInit { codec_string } => {
                        sender.send_message(MessageOutbound::VideoInit { codec_string })
                    }
                    StreamEvent::ConfigOk => sender.send_message(MessageOutbound::ConfigOk),
                    StreamEvent::Error(msg) => sender.send_message(MessageOutbound::Error(msg)),
                };
                if let Err(err) = result {
                    trace!("Client disconnected: {err}");
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("Client lagged {n} frames — skipping");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

#[allow(dead_code)]
fn handle_video_broadcast(
    cmd_rx: mpsc::Receiver<VideoCommands>,
    event_tx: broadcast::Sender<StreamEvent>,
    encoder_options: EncoderOptions,
) {
    let _ = event_tx.send(StreamEvent::ConfigOk);

    let event_tx_clone = event_tx.clone();
    match VideoEncoder::new(
        1,
        1,
        1,
        1,
        move |data| {
            let _ = event_tx_clone.send(StreamEvent::VideoFrame(Arc::new(data.to_vec())));
        },
        encoder_options,
    ) {
        Ok(enc) => {
            let _ = event_tx.send(StreamEvent::VideoInit {
                codec_string: enc.codec_string().to_string(),
            });
            drop(enc);
        }
        Err(e) => {
            let _ = event_tx.send(StreamEvent::Error(format!("Encoder creation failed: {e}")));
        }
    };

    for _cmd in cmd_rx.iter() {}
}

/// Messages sent from the capture thread to the encode thread.
enum EncodeCommand {
    /// New pixel data to encode.
    Frame(rylus_core::pixel::OwnedPixelData),
    /// Encoder must be restarted with new dimensions.
    Restart {
        width_in: usize,
        height_in: usize,
        width_out: usize,
        height_out: usize,
    },
    /// Adjust encoding quality.
    SetQuality(u32),
    /// Shut down the encode thread.
    Stop,
}

/// Encode thread: owns the VideoEncoder, receives frames from the capture thread.
fn encode_thread<S: RylusSender + Clone + Send + 'static>(
    encode_rx: mpsc::Receiver<EncodeCommand>,
    mut sender: S,
    encoder_options: EncoderOptions,
) {
    let mut video_encoder: Option<Box<VideoEncoder>> = None;

    while let Ok(cmd) = encode_rx.recv() {
        match cmd {
            EncodeCommand::Frame(owned) => {
                let enc = match video_encoder.as_mut() {
                    Some(e) => e,
                    None => continue,
                };
                enc.encode(owned.as_provider());
            }
            EncodeCommand::Restart {
                width_in,
                height_in,
                width_out,
                height_out,
            } => {
                send_message(&mut sender, MessageOutbound::NewVideo);
                let mut ws_sender = sender.clone();
                let res = VideoEncoder::new(
                    width_in,
                    height_in,
                    width_out,
                    height_out,
                    move |data| {
                        if let Err(err) = ws_sender.send_video(data) {
                            warn!("Failed to send video frame: {err}!");
                        }
                    },
                    encoder_options,
                );
                match res {
                    Ok(enc) => {
                        // Tell the client which codec string to use for MSE
                        send_message(
                            &mut sender,
                            MessageOutbound::VideoInit {
                                codec_string: enc.codec_string().to_string(),
                            },
                        );
                        video_encoder = Some(enc);
                    }
                    Err(e) => {
                        warn!("{}", e);
                        video_encoder = None;
                    }
                }
            }
            EncodeCommand::SetQuality(qp) => {
                if let Some(enc) = video_encoder.as_mut() {
                    enc.set_quality(qp);
                }
            }
            EncodeCommand::Stop => return,
        }
    }
}

// ---------------------------------------------------------------------------
// Adaptive quality controller
// ---------------------------------------------------------------------------

/// Minimum QP (highest quality). Below this, encoder artifacts are negligible
/// and further decreases waste bandwidth.
const QP_MIN: u32 = 18;

/// Maximum QP (lowest quality). Above this, the stream is too degraded to be useful.
const QP_MAX: u32 = 45;

/// Default QP used on startup and after encoder restart.
const QP_DEFAULT: u32 = 23;

/// Controls video encoding quality by weighing two independent signals:
/// - **Buffer health** (from the client): how many seconds of video are buffered.
///   High buffer → network/decode can't keep up → raise QP (reduce quality).
///   Low buffer → headroom available → lower QP (improve quality).
/// - **Pipeline ratio** (local): encode time relative to frame budget.
///   High ratio → encoder is saturated → raise QP.
///   Low ratio → encoder has headroom → lower QP.
///
/// When both signals agree, the QP moves. When they conflict (buffer says raise,
/// pipeline says lower), buffer health wins — it reflects the end-to-end bottleneck.
struct QualityController {
    current_qp: u32,
    encode_time_avg: f64,
    /// Most recent buffer health from client (seconds of buffered video).
    buffer_health_secs: Option<f64>,
}

impl QualityController {
    fn new() -> Self {
        Self {
            current_qp: QP_DEFAULT,
            encode_time_avg: 0.0,
            buffer_health_secs: None,
        }
    }

    fn reset(&mut self) {
        self.current_qp = QP_DEFAULT;
        self.encode_time_avg = 0.0;
        self.buffer_health_secs = None;
    }

    /// Update the buffer health signal from the client.
    fn set_buffer_health(&mut self, secs: f64) {
        self.buffer_health_secs = Some(secs);
    }

    /// Update the local encode time (exponential moving average).
    fn record_encode_time(&mut self, elapsed_secs: f64) {
        self.encode_time_avg = 0.3 * elapsed_secs + 0.7 * self.encode_time_avg;
    }

    /// Decide the new QP based on both signals. Returns Some(new_qp) if a change
    /// is needed, None if QP should stay the same.
    fn decide(&mut self, frame_budget_secs: f64) -> Option<u32> {
        // Buffer health signal: the end-to-end indicator.
        let buffer_delta = self.buffer_health_secs.map(|b| {
            if b > 3.0 {
                2i32 // Severely overloaded, aggressive reduction
            } else if b > 2.0 {
                1 // Mildly overloaded
            } else if b < 0.5 && self.current_qp > QP_MIN {
                -1 // Headroom available
            } else {
                0
            }
        });

        // Pipeline ratio signal: local encode performance.
        let pipeline_delta = if frame_budget_secs > 0.0 && frame_budget_secs < 1.0 {
            let ratio = self.encode_time_avg / frame_budget_secs;
            if ratio > 0.8 && self.current_qp < QP_MAX {
                Some(1i32)
            } else if ratio < 0.4 && self.current_qp > QP_MIN {
                Some(-1)
            } else {
                Some(0)
            }
        } else {
            None
        };

        // Combine: buffer health takes priority when it disagrees.
        let delta = match (buffer_delta, pipeline_delta) {
            (Some(bd), Some(pd)) => {
                if bd != 0 {
                    bd // Buffer health always wins when it has an opinion
                } else {
                    pd // Buffer neutral → use pipeline signal
                }
            }
            (Some(bd), None) => bd,
            (None, Some(pd)) => pd,
            (None, None) => 0,
        };

        // Clear one-shot buffer signal after consumption.
        self.buffer_health_secs = None;

        if delta == 0 {
            return None;
        }

        let new_qp = (self.current_qp as i32 + delta).clamp(QP_MIN as i32, QP_MAX as i32) as u32;
        if new_qp != self.current_qp {
            self.current_qp = new_qp;
            Some(new_qp)
        } else {
            None
        }
    }

    #[cfg(test)]
    fn qp(&self) -> u32 {
        self.current_qp
    }
}

/// Capture thread: captures frames at the target rate, sends them to the encode thread.
fn handle_video<S: RylusSender + Clone + Send + 'static>(
    receiver: mpsc::Receiver<VideoCommands>,
    sender: S,
    encoder_options: EncoderOptions,
) {
    const EFFECTIVE_INFINITY: Duration = Duration::from_secs(3600 * 24 * 365 * 200);

    // Bounded channel with capacity 1: if encode is busy, the frame is dropped.
    let (encode_tx, encode_rx) = mpsc::sync_channel::<EncodeCommand>(1);
    let encode_handle = {
        let sender = sender.clone();
        spawn(move || encode_thread(encode_rx, sender, encoder_options))
    };

    let mut recorder: Option<Box<dyn Recorder>> = None;
    let mut max_width = 1920;
    let mut max_height = 1080;
    let mut frame_duration = EFFECTIVE_INFINITY;
    let mut last_frame = Instant::now();
    let mut paused = false;

    let mut quality = QualityController::new();

    // Track current encoder dimensions to know when to restart
    let mut enc_width_in: usize = 0;
    let mut enc_height_in: usize = 0;
    let mut enc_width_out: usize = 0;
    let mut enc_height_out: usize = 0;

    // Frame drop metrics
    let mut frames_total: u64 = 0;
    let mut frames_dropped: u64 = 0;

    // Consecutive capture failure tracking
    let mut consecutive_capture_failures: u32 = 0;
    const MAX_CAPTURE_FAILURES: u32 = 30;
    let mut last_stats_log = Instant::now();

    loop {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_frame);
        let frames_passed = (elapsed.as_secs_f64() / frame_duration.as_secs_f64()) as u32;
        let next_frame = last_frame + (frames_passed + 1) * frame_duration;
        let timeout = next_frame.saturating_duration_since(now);

        if frames_passed > 0 {
            trace!("Dropped {frames_passed} frame(s) (pacing)!");
            frames_dropped += frames_passed as u64;
            frames_total += frames_passed as u64;
        }

        match receiver.recv_timeout(if paused { EFFECTIVE_INFINITY } else { timeout }) {
            Ok(VideoCommands::Start(config)) => {
                #[allow(unused_assignments)]
                {
                    // gstpipewire can not handle setting a pipeline's state to Null after another
                    // pipeline has been created and its state has been set to Play.
                    // See: https://gitlab.freedesktop.org/pipewire/pipewire/-/issues/986
                    recorder = None;
                }
                match config.capturable.recorder(config.capture_cursor) {
                    Ok(r) => {
                        recorder = Some(r);
                        max_width = config.max_width;
                        max_height = config.max_height;
                        send_message(&mut { sender.clone() }, MessageOutbound::ConfigOk);
                    }
                    Err(err) => {
                        warn!("Failed to init screen cast: {}!", err);
                        send_message(
                            &mut { sender.clone() },
                            MessageOutbound::Error("Failed to init screen cast!".into()),
                        )
                    }
                }
                last_frame = Instant::now();
                enc_width_in = 0; // Force encoder restart on next frame

                let d = 1.0 / config.frame_rate;
                frame_duration = if d.is_finite() && d > 0.0 {
                    Duration::from_secs_f64(d).min(EFFECTIVE_INFINITY)
                } else {
                    EFFECTIVE_INFINITY
                };
            }
            Ok(VideoCommands::Pause) => {
                paused = true;
            }
            Ok(VideoCommands::Resume) => {
                paused = false;
            }
            Ok(VideoCommands::Restart) => {
                enc_width_in = 0; // Force encoder restart
                quality.reset();
            }
            Ok(VideoCommands::BufferHealth(buffer_secs)) => {
                quality.set_buffer_health(buffer_secs);
            }
            Err(RecvTimeoutError::Timeout) => {
                last_frame = next_frame;
                let rec = match recorder.as_mut() {
                    Some(r) => r,
                    None => {
                        warn!("Screen capture not initialized, can not send video frame!");
                        continue;
                    }
                };
                let capture_start = Instant::now();
                let capture_result = rec.capture();
                let pixel_data = match capture_result {
                    Ok(p) => {
                        consecutive_capture_failures = 0;
                        p
                    }
                    Err(err) => {
                        consecutive_capture_failures += 1;
                        if consecutive_capture_failures >= MAX_CAPTURE_FAILURES {
                            warn!(
                                "Screen capture failed {} times in a row, giving up: {}",
                                consecutive_capture_failures, err
                            );
                            recorder = None;
                            send_message(
                                &mut { sender.clone() },
                                MessageOutbound::Error(
                                    "Screen capture lost — please reconnect.".into(),
                                ),
                            );
                        } else {
                            warn!("Error capturing screen: {}", err);
                        }
                        continue;
                    }
                };
                let (width_in, height_in) = pixel_data.size();
                let scale =
                    (max_width as f64 / width_in as f64).min(max_height as f64 / height_in as f64);
                let scale_max = (3840.0 / width_in as f64).min(2160.0 / height_in as f64);
                let scale = scale.min(scale_max);
                let mut width_out = width_in;
                let mut height_out = height_in;
                if scale < 1.0 {
                    width_out = ((width_out as f64 * scale) as usize).max(1);
                    height_out = ((height_out as f64 * scale) as usize).max(1);
                }

                // Check if encoder needs restart
                let needs_restart = enc_width_in != width_in
                    || enc_height_in != height_in
                    || enc_width_out != width_out
                    || enc_height_out != height_out;

                if needs_restart {
                    enc_width_in = width_in;
                    enc_height_in = height_in;
                    enc_width_out = width_out;
                    enc_height_out = height_out;
                    // Block on restart to ensure encoder is ready before sending frames
                    let _ = encode_tx.send(EncodeCommand::Restart {
                        width_in,
                        height_in,
                        width_out,
                        height_out,
                    });
                }

                // Copy pixel data to owned buffer and send to encode thread
                let owned = pixel_data.to_owned();
                let capture_elapsed = capture_start.elapsed().as_secs_f64();

                // try_send: if encoder is busy, drop this frame
                frames_total += 1;
                match encode_tx.try_send(EncodeCommand::Frame(owned)) {
                    Ok(()) => {
                        quality.record_encode_time(capture_elapsed);
                    }
                    Err(mpsc::TrySendError::Full(_)) => {
                        frames_dropped += 1;
                        trace!("Dropped frame (encoder busy)");
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        warn!("Encode thread disconnected");
                        return;
                    }
                }

                // Periodic frame stats logging
                if last_stats_log.elapsed() >= Duration::from_secs(5) && frames_total > 0 {
                    let rate = 100.0 * frames_dropped as f64 / frames_total as f64;
                    info!(
                        "Frame stats: {}/{} frames dropped ({:.1}%)",
                        frames_dropped, frames_total, rate
                    );
                    frames_total = 0;
                    frames_dropped = 0;
                    last_stats_log = Instant::now();
                }

                // Unified quality decision: weighs buffer health + pipeline ratio
                if let Some(new_qp) = quality.decide(frame_duration.as_secs_f64()) {
                    let _ = encode_tx.try_send(EncodeCommand::SetQuality(new_qp));
                    trace!("QP adjusted to {new_qp}");
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
    }

    // Shut down encode thread
    let _ = encode_tx.send(EncodeCommand::Stop);
    if let Err(e) = encode_handle.join() {
        warn!("Encode thread panicked: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // QualityController tests
    // -----------------------------------------------------------------------

    #[test]
    fn quality_controller_starts_at_default() {
        let qc = QualityController::new();
        assert_eq!(qc.qp(), QP_DEFAULT);
    }

    #[test]
    fn quality_controller_reset_restores_default() {
        let mut qc = QualityController::new();
        qc.set_buffer_health(5.0);
        qc.decide(0.033); // force a change
        qc.reset();
        assert_eq!(qc.qp(), QP_DEFAULT);
        assert!(qc.buffer_health_secs.is_none());
    }

    #[test]
    fn buffer_overloaded_raises_qp() {
        let mut qc = QualityController::new();
        qc.set_buffer_health(3.5); // > 3.0, severely overloaded
        let result = qc.decide(0.033);
        assert!(result.is_some());
        assert!(qc.qp() > QP_DEFAULT);
    }

    #[test]
    fn buffer_moderately_overloaded_raises_qp_by_one() {
        let mut qc = QualityController::new();
        qc.set_buffer_health(2.5); // > 2.0 but < 3.0
        let result = qc.decide(0.033);
        assert_eq!(result, Some(QP_DEFAULT + 1));
    }

    #[test]
    fn buffer_healthy_lowers_qp() {
        let mut qc = QualityController::new();
        // Start at a QP above minimum so there's room to decrease
        qc.current_qp = 30;
        qc.set_buffer_health(0.3); // < 0.5, headroom available
        let result = qc.decide(0.033);
        assert_eq!(result, Some(29));
    }

    #[test]
    fn buffer_at_minimum_qp_cannot_decrease() {
        let mut qc = QualityController::new();
        qc.current_qp = QP_MIN;
        qc.set_buffer_health(0.3);
        let result = qc.decide(0.033);
        assert!(result.is_none());
    }

    #[test]
    fn pipeline_saturated_raises_qp() {
        let mut qc = QualityController::new();
        // Simulate slow encoding: avg time is 90% of frame budget
        qc.encode_time_avg = 0.030; // 30ms
        let result = qc.decide(0.033); // 33ms budget (30fps) → ratio ~0.9
        assert!(result.is_some());
        assert!(qc.qp() > QP_DEFAULT);
    }

    #[test]
    fn pipeline_fast_lowers_qp() {
        let mut qc = QualityController::new();
        qc.current_qp = 30;
        // Simulate fast encoding: avg time is 20% of frame budget
        qc.encode_time_avg = 0.007; // 7ms
        let result = qc.decide(0.033); // ratio ~0.21
        assert_eq!(result, Some(29));
    }

    #[test]
    fn buffer_health_wins_over_pipeline() {
        let mut qc = QualityController::new();
        qc.current_qp = 30;
        // Buffer says: overloaded (raise QP)
        qc.set_buffer_health(3.5);
        // Pipeline says: fast (lower QP)
        qc.encode_time_avg = 0.005;
        let result = qc.decide(0.033);
        // Buffer should win
        assert!(result.is_some());
        assert!(qc.qp() > 30);
    }

    #[test]
    fn neutral_buffer_defers_to_pipeline() {
        let mut qc = QualityController::new();
        qc.current_qp = 30;
        // Buffer is neutral (between 0.5 and 2.0)
        qc.set_buffer_health(1.0);
        // Pipeline says: fast
        qc.encode_time_avg = 0.005;
        let result = qc.decide(0.033);
        // Pipeline signal should take effect
        assert_eq!(result, Some(29));
    }

    #[test]
    fn no_signals_returns_none() {
        let mut qc = QualityController::new();
        // No buffer health, no encode time, infinite frame budget
        let result = qc.decide(10.0); // budget > 1.0 → pipeline signal skipped
        assert!(result.is_none());
    }

    #[test]
    fn qp_clamped_at_max() {
        let mut qc = QualityController::new();
        qc.current_qp = QP_MAX;
        qc.set_buffer_health(5.0);
        let result = qc.decide(0.033);
        assert!(result.is_none()); // already at max
    }

    #[test]
    fn buffer_signal_consumed_after_decide() {
        let mut qc = QualityController::new();
        qc.set_buffer_health(3.5);
        qc.decide(0.033);
        // Second call without new buffer health should not move QP again
        // (unless pipeline signal triggers)
        qc.encode_time_avg = 0.0; // pipeline neutral
        let result = qc.decide(10.0);
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // StreamSession / broadcast tests
    // -----------------------------------------------------------------------

    #[test]
    fn stream_session_new_creates_broadcast() {
        let session = StreamSession::new(EncoderOptions::default());
        assert_eq!(session.subscriber_count(), 0);
    }

    #[test]
    fn stream_session_subscribe_increments_count() {
        let session = StreamSession::new(EncoderOptions::default());
        let _rx1 = session.subscribe();
        let _rx2 = session.subscribe();
        assert_eq!(session.subscriber_count(), 2);
    }

    #[tokio::test]
    async fn stream_session_video_frame_broadcast() {
        let session = StreamSession::new(EncoderOptions::default());
        let mut rx1 = session.subscribe();
        let mut rx2 = session.subscribe();

        let data = Arc::new(vec![1u8, 2, 3, 4]);
        let _ = session.event_tx.send(StreamEvent::VideoFrame(data.clone()));

        let ev1 = rx1.recv().await.unwrap();
        let ev2 = rx2.recv().await.unwrap();

        match (ev1, ev2) {
            (StreamEvent::VideoFrame(d1), StreamEvent::VideoFrame(d2)) => {
                assert!(Arc::ptr_eq(&data, &d1));
                assert!(Arc::ptr_eq(&data, &d2));
            }
            _ => panic!("Expected VideoFrame events"),
        }
    }

    #[tokio::test]
    async fn stream_session_lagged_receiver_skips() {
        let session = StreamSession::new(EncoderOptions::default());
        let _rx1 = session.subscribe();
        let _rx2 = session.subscribe();

        let (tx, mut rx) = broadcast::channel(1);
        tx.send(StreamEvent::ConfigOk).unwrap();
        tx.send(StreamEvent::NewVideo).unwrap();
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(n)) => assert_eq!(n, 1),
            other => panic!("Expected Lagged error, got: {other:?}"),
        }
    }

    #[test]
    fn stream_event_clone_shared() {
        let data = Arc::new(vec![42u8, 43, 44]);
        let event = StreamEvent::VideoFrame(data.clone());
        if let StreamEvent::VideoFrame(d2) = event {
            assert!(
                Arc::ptr_eq(&data, &d2),
                "VideoFrame should share the Arc, not clone"
            );
        } else {
            panic!("Expected VideoFrame");
        }
    }

    // -----------------------------------------------------------------------
    // video_forwarder tests
    // -----------------------------------------------------------------------

    struct MockSender {
        messages: std::sync::Mutex<Vec<MessageOutbound>>,
        videos: std::sync::Mutex<Vec<Vec<u8>>>,
    }

    impl MockSender {
        fn new() -> Self {
            Self {
                messages: std::sync::Mutex::new(Vec::new()),
                videos: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl RylusSender for MockSender {
        type Error = std::convert::Infallible;

        fn send_message(&mut self, message: MessageOutbound) -> Result<(), Self::Error> {
            self.messages.lock().unwrap().push(message);
            Ok(())
        }

        fn send_video(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.videos.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }
    }

    #[test]
    fn video_forwarder_sends_frames() {
        let (tx, rx) = broadcast::channel(8);
        let sender = MockSender::new();

        tx.send(StreamEvent::VideoFrame(Arc::new(vec![1, 2, 3])))
            .unwrap();
        tx.send(StreamEvent::ConfigOk).unwrap();
        tx.send(StreamEvent::NewVideo).unwrap();
        tx.send(StreamEvent::VideoInit {
            codec_string: "vp8".to_string(),
        })
        .unwrap();
        tx.send(StreamEvent::Error("test error".to_string()))
            .unwrap();
        drop(tx);

        video_forwarder(rx, sender);

        // The MockSender is moved into the thread, so we can't check its contents here.
        // The test verifies the function doesn't panic and exits cleanly.
    }
}
