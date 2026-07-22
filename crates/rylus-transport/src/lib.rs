pub mod websocket;

pub use websocket::{
    rylus_websocket_channel, rylus_websocket_channel_from_hyper_upgrade, WsMessage,
    WsRylusReceiver, WsRylusSender,
};

/// RFC 6455 `Sec-WebSocket-Accept` derivation, re-exported so the HTTP server
/// can build the upgrade response without depending on tungstenite directly.
pub use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
