use fastwebsockets::{FragmentCollectorRead, Frame, OpCode, WebSocket, WebSocketError};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::channel;
use tracing::warn;

use rylus_core::protocol::{MessageInbound, MessageOutbound, RylusReceiver, RylusSender};

/// Maximum size of a text WebSocket frame (control messages).
/// Binary frames (video) are not limited.
const MAX_TEXT_FRAME_SIZE: usize = 64 * 1024; // 64 KB

/// Idle timeout: close the connection if no message is received within this duration.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Channel buffer capacity for both inbound and outbound WebSocket messages.
/// Provides backpressure: senders block when the buffer is full.
const CHANNEL_BUFFER_SIZE: usize = 32;

pub struct WsRylusReceiver {
    recv: tokio::sync::mpsc::Receiver<MessageInbound>,
}

impl Iterator for WsRylusReceiver {
    type Item = Result<MessageInbound, Infallible>;

    fn next(&mut self) -> Option<Self::Item> {
        self.recv.blocking_recv().map(Ok)
    }
}

impl RylusReceiver for WsRylusReceiver {
    type Error = Infallible;
}

pub enum WsMessage {
    Frame(Frame<'static>),
    Video(Vec<u8>),
    MessageOutbound(MessageOutbound),
}

// SAFETY: WsMessage contains Frame<'static> which holds a Payload (Bytes or Vec<u8>),
// both of which are Send. The other variants (Video, MessageOutbound) are trivially Send.
// Frame is not marked Send by fastwebsockets because it can hold borrowed data, but
// we only construct Frame<'static> with owned data here.
unsafe impl Send for WsMessage {}

#[derive(Clone)]
pub struct WsRylusSender {
    sender: tokio::sync::mpsc::Sender<WsMessage>,
}

impl RylusSender for WsRylusSender {
    type Error = tokio::sync::mpsc::error::SendError<WsMessage>;

    fn send_message(&mut self, message: MessageOutbound) -> Result<(), Self::Error> {
        self.sender
            .blocking_send(WsMessage::MessageOutbound(message))
    }

    fn send_video(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.sender.blocking_send(WsMessage::Video(bytes.to_vec()))
    }
}

pub fn rylus_websocket_channel(
    websocket: WebSocket<TokioIo<Upgraded>>,
    semaphore_shutdown: Arc<tokio::sync::Semaphore>,
) -> (WsRylusSender, WsRylusReceiver) {
    let (rx, mut tx) = websocket.split(tokio::io::split);

    let mut rx = FragmentCollectorRead::new(rx);

    let (sender_inbound, receiver_inbound) = channel::<MessageInbound>(CHANNEL_BUFFER_SIZE);
    let (sender_outbound, mut receiver_outbound) = channel::<WsMessage>(CHANNEL_BUFFER_SIZE);

    {
        let sender_outbound = sender_outbound.clone();
        tokio::spawn(async move {
            let mut send_fn = |frame| async {
                if let Err(err) = sender_outbound.send(WsMessage::Frame(frame)).await {
                    warn!("Failed to send websocket frame while receiving fragmented frame: {err}.")
                };
                Ok(())
            };

            loop {
                let fut = rx.read_frame::<_, WebSocketError>(&mut send_fn);

                let frame = tokio::select! {
                    _ = semaphore_shutdown.acquire() => break,
                    _ = tokio::time::sleep(IDLE_TIMEOUT) => {
                        warn!("WebSocket idle timeout ({IDLE_TIMEOUT:?}) — closing connection.");
                        break;
                    },
                    frame = fut => match frame {
                        Ok(frame) => frame,
                        Err(err) => {
                            warn!("Invalid websocket frame: {err}.");
                            break;
                        },
                    },
                };
                match frame.opcode {
                    OpCode::Close => break,
                    OpCode::Text => {
                        if frame.payload.len() > MAX_TEXT_FRAME_SIZE {
                            warn!(
                                "Text frame too large ({} bytes, max {MAX_TEXT_FRAME_SIZE}) — dropping.",
                                frame.payload.len()
                            );
                            continue;
                        }
                        match serde_json::from_slice(&frame.payload) {
                            Ok(msg) => {
                                if let Err(err) = sender_inbound.send(msg).await {
                                    warn!("Failed to forward inbound message to RylusClientHandler: {err}.");
                                }
                            }
                            Err(err) => warn!("Failed to parse message: {err}"),
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    tokio::spawn(async move {
        loop {
            let msg = if let Some(msg) = receiver_outbound.recv().await {
                msg
            } else {
                break;
            };

            match msg {
                WsMessage::Frame(frame) => {
                    if let Err(err) = tx.write_frame(frame).await {
                        if let WebSocketError::ConnectionClosed = err {
                            break;
                        }
                        warn!("Failed to send frame: {err}");
                    }
                }
                WsMessage::Video(data) => {
                    if let Err(err) = tx.write_frame(Frame::binary(data.into())).await {
                        if let WebSocketError::ConnectionClosed = err {
                            break;
                        }
                        warn!("Failed to send video frame: {err}");
                    }
                }
                WsMessage::MessageOutbound(msg) => {
                    let json_string = match serde_json::to_string(&msg) {
                        Ok(s) => s,
                        Err(err) => {
                            warn!("Failed to serialize outbound message: {err}");
                            continue;
                        }
                    };
                    let data = json_string.as_bytes();
                    if let Err(err) = tx.write_frame(Frame::text(data.into())).await {
                        if let WebSocketError::ConnectionClosed = err {
                            break;
                        }
                        warn!("Failed to send outbound message: {err}");
                    }
                }
            }
        }
    });

    (
        WsRylusSender {
            sender: sender_outbound,
        },
        WsRylusReceiver {
            recv: receiver_inbound,
        },
    )
}
