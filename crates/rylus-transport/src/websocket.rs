use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc::channel;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
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
    /// A raw tungstenite [`Message`] to forward directly over the WebSocket.
    Raw(Message),
    /// Video frame bytes (sent as a binary WebSocket frame).
    Video(Vec<u8>),
    /// Protocol message (serialized as JSON text WebSocket frame).
    MessageOutbound(MessageOutbound),
}

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

/// Split a [`WebSocketStream`] into a sender/receiver pair.
///
/// The `S` parameter is typically `TokioIo<Upgraded>` for HTTP-upgraded connections
/// or `TcpStream` for raw TCP WebSocket connections.
///
/// This function is synchronous — it spawns two tokio tasks internally:
/// one for receiving inbound messages and one for sending outbound messages.
pub async fn rylus_websocket_channel_from_hyper_upgrade(
    upgraded: hyper::upgrade::Upgraded,
    semaphore_shutdown: Arc<tokio::sync::Semaphore>,
) -> (WsRylusSender, WsRylusReceiver) {
    let ws_stream = WebSocketStream::from_raw_socket(
        TokioIo::new(upgraded),
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;
    rylus_websocket_channel(ws_stream, semaphore_shutdown)
}

pub fn rylus_websocket_channel<S>(
    websocket: WebSocketStream<S>,
    semaphore_shutdown: Arc<tokio::sync::Semaphore>,
) -> (WsRylusSender, WsRylusReceiver)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut write, mut read) = websocket.split();

    let (sender_inbound, receiver_inbound) = channel::<MessageInbound>(CHANNEL_BUFFER_SIZE);
    let (sender_outbound, mut receiver_outbound) = channel::<WsMessage>(CHANNEL_BUFFER_SIZE);

    tokio::spawn(async move {
        loop {
            let msg = tokio::select! {
                _ = semaphore_shutdown.acquire() => break,
                _ = tokio::time::sleep(IDLE_TIMEOUT) => {
                    warn!("WebSocket idle timeout ({IDLE_TIMEOUT:?}) — closing connection.");
                    break;
                },
                msg = read.next() => match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(err)) => {
                        warn!("WebSocket read error: {err}.");
                        break;
                    }
                    None => break,
                },
            };

            match msg {
                Message::Close(_) => break,
                Message::Text(text) => {
                    if text.len() > MAX_TEXT_FRAME_SIZE {
                        warn!(
                            "Text frame too large ({} bytes, max {MAX_TEXT_FRAME_SIZE}) — dropping.",
                            text.len()
                        );
                        continue;
                    }
                    match serde_json::from_str(&text) {
                        Ok(msg) => {
                            if let Err(err) = sender_inbound.send(msg).await {
                                warn!("Failed to forward inbound message to RylusClientHandler: {err}.");
                            }
                        }
                        Err(err) => warn!("Failed to parse message: {err}"),
                    }
                }
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    });

    tokio::spawn(async move {
        loop {
            let msg = if let Some(msg) = receiver_outbound.recv().await {
                msg
            } else {
                break;
            };

            let result = match msg {
                WsMessage::Raw(message) => write.send(message).await,
                WsMessage::Video(data) => write.send(Message::Binary(data.into())).await,
                WsMessage::MessageOutbound(msg) => {
                    let json_string = match serde_json::to_string(&msg) {
                        Ok(s) => s,
                        Err(err) => {
                            warn!("Failed to serialize outbound message: {err}");
                            continue;
                        }
                    };
                    write.send(Message::Text(json_string.into())).await
                }
            };

            if let Err(err) = result {
                warn!("Failed to send WebSocket message: {err}");
                break;
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

/// Type alias for the most common server-side usage: an HTTP-upgraded WebSocket.
pub type UpgradedWebSocket = WebSocketStream<TokioIo<Upgraded>>;

/// Type alias for a raw TCP WebSocket connection.
pub type TcpWebSocket = WebSocketStream<TcpStream>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that tungstenite Message types can be constructed for all send paths.
    #[test]
    fn message_types_are_constructible() {
        let binary_msg = Message::Binary(vec![0x00, 0x01, 0x02, 0x03].into());
        assert!(binary_msg.is_binary());
        assert_eq!(binary_msg.into_data(), vec![0x00, 0x01, 0x02, 0x03]);

        let text_msg = Message::Text(r#""Heartbeat""#.into());
        assert!(text_msg.is_text());
        assert_eq!(
            text_msg.into_text().unwrap().as_str(),
            r#""Heartbeat""#
        );

        let close_msg = Message::Close(None);
        assert!(close_msg.is_close());

        let ping = Message::Ping(vec![].into());
        assert!(ping.is_ping());
        let pong = Message::Pong(vec![].into());
        assert!(pong.is_pong());
    }

    /// Test that WsMessage enum variants can be constructed and pattern-matched.
    #[test]
    fn ws_message_variants_are_constructible() {
        let raw = WsMessage::Raw(Message::Text("test".into()));
        let video = WsMessage::Video(vec![0xFF; 1024]);
        let outbound =
            WsMessage::MessageOutbound(rylus_core::protocol::MessageOutbound::Error(
                "test error".into(),
            ));

        match raw {
            WsMessage::Raw(_) => {}
            WsMessage::Video(_) | WsMessage::MessageOutbound(_) => {
                panic!("expected Raw variant")
            }
        }
        match video {
            WsMessage::Video(_) => {}
            WsMessage::Raw(_) | WsMessage::MessageOutbound(_) => {
                panic!("expected Video variant")
            }
        }
        match outbound {
            WsMessage::MessageOutbound(_) => {}
            WsMessage::Raw(_) | WsMessage::Video(_) => {
                panic!("expected MessageOutbound variant")
            }
        }
    }

    /// Test that WsRylusSender can be cloned.
    #[test]
    fn ws_rylus_sender_is_cloneable() {
        let (tx, _rx) = channel::<WsMessage>(4);
        let sender = WsRylusSender { sender: tx };
        let _clone = sender.clone();
    }

    /// Test that WsRylusSender implements RylusSender trait.
    #[test]
    fn ws_rylus_sender_implements_rylus_sender() {
        fn assert_rylus_sender<T: RylusSender>() {}
        assert_rylus_sender::<WsRylusSender>();
    }

    /// Test that WsRylusReceiver implements RylusReceiver trait.
    #[test]
    fn ws_rylus_receiver_implements_rylus_receiver() {
        fn assert_rylus_receiver<T: RylusReceiver>() {}
        assert_rylus_receiver::<WsRylusReceiver>();
    }

    /// Test roundtrip: server receives a text Heartbeat from client.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn websocket_roundtrip_text_heartbeat() {
        let (client_io, server_io) = tokio::io::duplex(65536);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

        let client_handle = tokio::spawn(async move {
            let (ws_stream, _) =
                tokio_tungstenite::client_async("ws://localhost", client_io)
                    .await
                    .expect("client connect should succeed");
            let (mut write, _read) = ws_stream.split();
            write
                .send(Message::Text(
                    r#""Heartbeat""#.into(),
                ))
                .await
                .expect("client send should succeed");
            done_rx.await.ok();
        });

        let ws = tokio_tungstenite::accept_async(server_io)
            .await
            .expect("server accept should succeed");
        let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
        let (_sender, receiver) = rylus_websocket_channel(ws, semaphore);

        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let mut rx = receiver;
            let result = rx.next();
            let _ = result_tx.send(result);
        });

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            result_rx,
        )
        .await
        .expect("receive should not timeout")
        .expect("result channel should not close");

        done_tx.send(()).ok();

        assert!(result.is_some(), "should receive a message");
        match result.unwrap().unwrap() {
            MessageInbound::Heartbeat => {}
            other => panic!("expected Heartbeat, got: {other:?}"),
        }

        client_handle.await.expect("client task should complete");
    }

    /// Test that WsRylusSender.send_video produces a binary frame on the wire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sender_video_produces_binary_frame() {
        let (client_io, server_io) = tokio::io::duplex(65536);

        // Spawn client FIRST — it will read what the server sends.
        let client_handle = tokio::spawn(async move {
            let (ws_stream, _) =
                tokio_tungstenite::client_async("ws://localhost", client_io)
                    .await
                    .expect("client connect should succeed");
            let (_write, mut read) = ws_stream.split();
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                read.next(),
            )
            .await
            .expect("client read should not timeout")
            .expect("should receive a message")
            .expect("message should be ok");
            assert!(msg.is_binary(), "expected binary frame, got: {msg:?}");
            assert_eq!(
                msg.into_data(),
                vec![0x01, 0x02, 0x03],
                "binary payload mismatch"
            );
        });

        let ws = tokio_tungstenite::accept_async(server_io)
            .await
            .expect("server accept should succeed");
        let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
        let (mut sender, _receiver) = rylus_websocket_channel(ws, semaphore);

        tokio::task::spawn_blocking(move || {
            sender
                .send_video(&[0x01, 0x02, 0x03])
                .expect("send_video should succeed");
        })
        .await
        .expect("sender task should complete");

        client_handle.await.expect("client task should complete");
    }

    /// Test that WsRylusSender.send_message produces a JSON text frame on the wire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sender_message_produces_text_frame() {
        let (client_io, server_io) = tokio::io::duplex(65536);

        // Spawn client FIRST — it will read what the server sends.
        let client_handle = tokio::spawn(async move {
            let (ws_stream, _) =
                tokio_tungstenite::client_async("ws://localhost", client_io)
                    .await
                    .expect("client connect should succeed");
            let (_write, mut read) = ws_stream.split();
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                read.next(),
            )
            .await
            .expect("client read should not timeout")
            .expect("should receive a message")
            .expect("message should be ok");
            assert!(msg.is_text(), "expected text frame, got: {msg:?}");
            let text = msg.into_text().unwrap();
            assert!(
                text.as_str().contains("Error"),
                "expected JSON with Error, got: {text}"
            );
        });

        let ws = tokio_tungstenite::accept_async(server_io)
            .await
            .expect("server accept should succeed");
        let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
        let (mut sender, _receiver) = rylus_websocket_channel(ws, semaphore);

        tokio::task::spawn_blocking(move || {
            sender
                .send_message(rylus_core::protocol::MessageOutbound::Error(
                    "test".into(),
                ))
                .expect("send_message should succeed");
        })
        .await
        .expect("sender task should complete");

        client_handle.await.expect("client task should complete");
    }

    /// Test that shutdown semaphore terminates the receive loop.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_semaphore_closes_connection() {
        let (client_io, server_io) = tokio::io::duplex(4096);

        let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
        let shutdown_sem = semaphore.clone();

        let client_handle = tokio::spawn(async move {
            let (ws_stream, _) =
                tokio_tungstenite::client_async("ws://localhost", client_io)
                    .await
                    .expect("client connect should succeed");
            drop(ws_stream);
        });

        let ws = tokio_tungstenite::accept_async(server_io)
            .await
            .expect("server accept should succeed");
        let (_sender, mut receiver) = rylus_websocket_channel(ws, semaphore);

        let handle = tokio::task::spawn_blocking(move || {
            let mut count = 0u32;
            while receiver.next().is_some() {
                count += 1;
            }
            count
        });

        shutdown_sem.add_permits(1);

        let _count = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            handle,
        )
        .await
        .expect("shutdown should complete within timeout")
        .expect("spawn_blocking should not panic");

        client_handle.await.expect("client task should complete");
    }
}
