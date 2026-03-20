use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::thread::{spawn, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{error, info, trace, warn};

use rylus_capture::{get_capturables, Capturable, Recorder};
use rylus_core::error::CErrorCode;
use rylus_core::protocol::{
    ClientConfiguration, Hello, KeyboardEvent, MessageInbound, MessageOutbound, PointerEvent,
    RylusReceiver, RylusSender, WheelEvent, PROTOCOL_VERSION,
};
use rylus_encode::{EncoderOptions, VideoEncoder};
use rylus_input::device::{InputDevice, InputDeviceType};

struct VideoConfig {
    capturable: Box<dyn Capturable>,
    capture_cursor: bool,
    max_width: usize,
    max_height: usize,
    frame_rate: f64,
}

enum VideoCommands {
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
                        MessageInbound::Heartbeat => {} // keepalive; receipt resets idle timer
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
                    Ok(enc) => video_encoder = Some(enc),
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

    // Adaptive quality state
    let mut current_qp: u32 = 23;
    let mut encode_time_avg: f64 = 0.0;

    // Track current encoder dimensions to know when to restart
    let mut enc_width_in: usize = 0;
    let mut enc_height_in: usize = 0;
    let mut enc_width_out: usize = 0;
    let mut enc_height_out: usize = 0;

    // Frame drop metrics
    let mut frames_total: u64 = 0;
    let mut frames_dropped: u64 = 0;
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
                current_qp = 23;
                encode_time_avg = 0.0;
            }
            Ok(VideoCommands::BufferHealth(buffer_secs)) => {
                let new_qp = if buffer_secs > 3.0 {
                    (current_qp + 2).min(45)
                } else if buffer_secs > 2.0 {
                    (current_qp + 1).min(45)
                } else if buffer_secs < 0.5 && current_qp > 18 {
                    current_qp - 1
                } else {
                    current_qp
                };
                if new_qp != current_qp {
                    current_qp = new_qp;
                    let _ = encode_tx.try_send(EncodeCommand::SetQuality(current_qp));
                    trace!(
                        "QP adjusted to {current_qp} based on buffer health ({buffer_secs:.1}s)"
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                last_frame = next_frame;
                let recorder = match recorder.as_mut() {
                    Some(r) => r,
                    None => {
                        warn!("Screen capture not initialized, can not send video frame!");
                        continue;
                    }
                };
                let capture_start = Instant::now();
                let pixel_data = match recorder.capture() {
                    Ok(p) => p,
                    Err(err) => {
                        warn!("Error capturing screen: {}", err);
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
                        // Track capture time for adaptive quality
                        encode_time_avg = 0.3 * capture_elapsed + 0.7 * encode_time_avg;
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

                // Adapt quality based on overall pipeline performance
                let frame_budget = frame_duration.as_secs_f64();
                if frame_budget > 0.0 && frame_budget < 1.0 {
                    let ratio = encode_time_avg / frame_budget;
                    let new_qp = if ratio > 0.8 && current_qp < 45 {
                        current_qp + 1
                    } else if ratio < 0.4 && current_qp > 18 {
                        current_qp - 1
                    } else {
                        current_qp
                    };
                    if new_qp != current_qp {
                        current_qp = new_qp;
                        let _ = encode_tx.try_send(EncodeCommand::SetQuality(current_qp));
                        trace!("QP adjusted to {current_qp} (pipeline ratio: {ratio:.2})");
                    }
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
