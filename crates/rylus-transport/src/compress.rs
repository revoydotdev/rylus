//! Application-level deflate compression for WebSocket text messages.
//!
//! Binary frames (video) are NOT compressed — video codecs already compress.
//!
//! TODO(#4.5): When migrating from `fastwebsockets` to a library supporting
//! `permessage-deflate` (RFC 7692), replace this with protocol-level compression.

#[cfg(feature = "compression")]
use flate2::write::{DeflateDecoder, DeflateEncoder};
#[cfg(feature = "compression")]
use flate2::Compression;
#[cfg(feature = "compression")]
use std::io::Write;

/// 1-byte prefix identifying compressed binary frames.
/// 0x01 is safe — it never appears as the first byte of valid JSON.
pub const COMPRESSED_FRAME_PREFIX: u8 = 0x01;

#[cfg(feature = "compression")]
pub fn compress_text(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(data)
        .expect("write to vec encoder should not fail");
    let compressed = encoder
        .finish()
        .expect("finish compression should not fail");
    let mut out = Vec::with_capacity(1 + compressed.len());
    out.push(COMPRESSED_FRAME_PREFIX);
    out.extend_from_slice(&compressed);
    out
}

#[cfg(feature = "compression")]
pub fn decompress_text(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = DeflateDecoder::new(Vec::new());
    decoder.write_all(data)?;
    decoder.finish()
}

#[inline]
pub fn is_compressed_frame(data: &[u8]) -> bool {
    data.first().copied() == Some(COMPRESSED_FRAME_PREFIX)
}

#[inline]
pub fn strip_compression_prefix(data: &[u8]) -> &[u8] {
    if is_compressed_frame(data) {
        &data[1..]
    } else {
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_compressed_frame_detects_prefix() {
        let mut data = vec![COMPRESSED_FRAME_PREFIX, 0x78, 0x9C];
        assert!(is_compressed_frame(&data));

        data[0] = 0x00;
        assert!(!is_compressed_frame(&data));

        assert!(!is_compressed_frame(&[]));
    }

    #[test]
    fn strip_compression_prefix_removes_prefix_byte() {
        let data = vec![COMPRESSED_FRAME_PREFIX, 0xAA, 0xBB, 0xCC];
        assert_eq!(strip_compression_prefix(&data), &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn strip_compression_prefix_passthrough_when_uncompressed() {
        let data = vec![0x00, 0xAA, 0xBB];
        assert_eq!(strip_compression_prefix(&data), &[0x00, 0xAA, 0xBB]);
    }

    #[cfg(feature = "compression")]
    const SMALL_JSON: &str = r#"{"type":"Heartbeat"}"#;
    #[cfg(feature = "compression")]
    const LARGER_JSON: &str = r#"{"type":"PointerEvent","data":{"x":123.456,"y":789.012,"pressure":0.5,"buttons":1,"pointerType":"pen","tiltX":0,"tiltY":0}}{"type":"PointerEvent","data":{"x":124.0,"y":789.5,"pressure":0.6,"buttons":1,"pointerType":"pen","tiltX":1,"tiltY":2}}"#;

    #[cfg(feature = "compression")]
    #[test]
    fn compress_decompress_roundtrip_small_json() {
        let original = SMALL_JSON.as_bytes();
        let compressed = compress_text(original);
        assert!(compressed.len() > 1);
        assert_eq!(compressed[0], COMPRESSED_FRAME_PREFIX);

        let decompressed = decompress_text(&compressed[1..]).expect("decompress should succeed");
        assert_eq!(decompressed, original);
    }

    #[cfg(feature = "compression")]
    #[test]
    fn compress_decompress_roundtrip_larger_json() {
        let original = LARGER_JSON.as_bytes();
        let compressed = compress_text(original);

        assert!(
            compressed.len() < original.len(),
            "compressed {} < original {}",
            compressed.len(),
            original.len()
        );

        let decompressed = decompress_text(&compressed[1..]).expect("decompress should succeed");
        assert_eq!(decompressed, original);
    }

    #[cfg(feature = "compression")]
    #[test]
    fn compressed_frame_has_valid_prefix() {
        let compressed = compress_text(b"hello");
        assert!(is_compressed_frame(&compressed));
    }

    #[cfg(feature = "compression")]
    #[test]
    fn highly_repetitive_data_compresses_below_ten_percent() {
        let repetitive = "A".repeat(10_000);
        let compressed = compress_text(repetitive.as_bytes());
        assert!(
            compressed.len() < repetitive.len() / 10,
            "repetitive data should compress to <10%"
        );

        let decompressed = decompress_text(&compressed[1..]).expect("decompress should succeed");
        assert_eq!(decompressed, repetitive.as_bytes());
    }

    #[cfg(feature = "compression")]
    #[test]
    fn decompress_invalid_data_returns_error() {
        assert!(decompress_text(&[0xFF, 0xFE, 0xFD, 0xFC]).is_err());
    }

    #[cfg(feature = "compression")]
    #[test]
    fn decompress_empty_input_does_not_panic() {
        let _ = decompress_text(&[]);
    }

    #[cfg(feature = "compression")]
    #[test]
    fn realistic_protocol_messages_roundtrip() {
        let messages = vec![
            r#"{"Hello":{"version":"0.17.0"}}"#,
            r#"{"PointerEvent":{"x":100.5,"y":200.3,"pressure":0.75,"buttons":1}}"#,
            r#"{"BatchedPointerEvents":[{"x":1,"y":2},{"x":3,"y":4},{"x":5,"y":6},{"x":7,"y":8},{"x":9,"y":10}]}"#,
            r#"{"KeyboardEvent":{"code":"KeyA","key":"a","type":"keydown","modifiers":0}}"#,
        ];

        for msg in &messages {
            let original = msg.as_bytes();
            let compressed = compress_text(original);
            let decompressed =
                decompress_text(&compressed[1..]).expect("decompress should succeed");
            assert_eq!(decompressed, original, "roundtrip failed for: {msg}");
            assert!(
                compressed.len() < original.len() * 2,
                "compressed shouldn't be >2x original"
            );
        }
    }

    #[test]
    fn compression_prefix_byte_does_not_conflict_with_json_start() {
        assert_ne!(COMPRESSED_FRAME_PREFIX, b'{');
        assert_ne!(COMPRESSED_FRAME_PREFIX, b'[');
        assert_ne!(COMPRESSED_FRAME_PREFIX, b'"');
        assert_ne!(COMPRESSED_FRAME_PREFIX, b' ');
    }
}
