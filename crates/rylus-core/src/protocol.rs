use serde::{Deserialize, Deserializer, Serialize};

/// Current protocol version.
///
/// Increment when adding new message types or changing semantics.
/// Clients and server negotiate on the minimum of their versions.
pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Hello {
    pub protocol_version: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientConfiguration {
    #[cfg(target_os = "linux")]
    pub uinput_support: bool,
    pub capturable_id: usize,
    pub capture_cursor: bool,
    pub max_width: usize,
    pub max_height: usize,
    pub client_name: Option<String>,
    pub frame_rate: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BufferHealth {
    /// Seconds of buffered video the client currently holds.
    pub buffer_seconds: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageInbound {
    Hello(Hello),
    PointerEvent(PointerEvent),
    WheelEvent(WheelEvent),
    KeyboardEvent(KeyboardEvent),
    GetCapturableList,
    Config(ClientConfiguration),
    PauseVideo,
    ResumeVideo,
    RestartVideo,
    ChooseCustomInputAreas,
    BufferHealth(BufferHealth),
    Heartbeat,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageOutbound {
    Hello(Hello),
    CapturableList(Vec<String>),
    NewVideo,
    ConfigOk,
    CustomInputAreas(CustomInputAreas),
    ConfigError(String),
    Error(String),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct CustomInputAreas {
    pub mouse: Option<Rect>,
    pub touch: Option<Rect>,
    pub pen: Option<Rect>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PointerType {
    #[serde(rename = "")]
    Unknown,
    #[serde(rename = "mouse")]
    Mouse,
    #[serde(rename = "pen")]
    Pen,
    #[serde(rename = "touch")]
    Touch,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PointerEventType {
    #[serde(rename = "pointerdown")]
    DOWN,
    #[serde(rename = "pointerup")]
    UP,
    #[serde(rename = "pointercancel")]
    CANCEL,
    #[serde(rename = "pointermove")]
    MOVE,
    #[serde(rename = "pointerover")]
    OVER,
    #[serde(rename = "pointerenter")]
    ENTER,
    #[serde(rename = "pointerleave")]
    LEAVE,
    #[serde(rename = "pointerout")]
    OUT,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KeyboardEventType {
    #[serde(rename = "down")]
    DOWN,
    #[serde(rename = "up")]
    UP,
    #[serde(rename = "repeat")]
    REPEAT,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum KeyboardLocation {
    STANDARD,
    LEFT,
    RIGHT,
    NUMPAD,
}

fn location_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<KeyboardLocation, D::Error> {
    let code: u8 = Deserialize::deserialize(deserializer)?;
    match code {
        0 => Ok(KeyboardLocation::STANDARD),
        1 => Ok(KeyboardLocation::LEFT),
        2 => Ok(KeyboardLocation::RIGHT),
        3 => Ok(KeyboardLocation::NUMPAD),
        _ => Err(serde::de::Error::custom(
            "Failed to parse keyboard location code.",
        )),
    }
}

bitflags! {
    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
    pub struct Button: u8 {
        const NONE = 0b0000_0000;
        const PRIMARY = 0b0000_0001;
        const SECONDARY = 0b0000_0010;
        const AUXILARY = 0b0000_0100;
        const FOURTH = 0b0000_1000;
        const FIFTH = 0b0001_0000;
        const ERASER = 0b0010_0000;
    }
}

fn button_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Button, D::Error> {
    let bits: u8 = Deserialize::deserialize(deserializer)?;
    Button::from_bits(bits).ok_or_else(|| serde::de::Error::custom("Failed to parse button code."))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeyboardEvent {
    pub event_type: KeyboardEventType,
    pub code: String,
    pub key: String,
    #[serde(deserialize_with = "location_from")]
    pub location: KeyboardLocation,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub meta: bool,
}

/// Pointer event from the client.
///
/// Field ranges:
/// - `x`, `y`: 0.0–1.0 (relative to the capture area)
/// - `pressure`: 0.0–1.0 (0.0 = no pressure data)
/// - `tilt_x`, `tilt_y`: -90..90 degrees
/// - `width`, `height`: touch contact size, relative to capture area
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PointerEvent {
    pub event_type: PointerEventType,
    pub pointer_id: i64,
    pub timestamp: u64,
    pub is_primary: bool,
    pub pointer_type: PointerType,
    #[serde(deserialize_with = "button_from")]
    pub button: Button,
    #[serde(deserialize_with = "button_from")]
    pub buttons: Button,
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub tilt_x: i32,
    pub tilt_y: i32,
    pub twist: i32,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WheelEvent {
    pub dx: i32,
    pub dy: i32,
    pub timestamp: u64,
}

pub trait RylusSender {
    type Error: std::error::Error;
    fn send_message(&mut self, message: MessageOutbound) -> Result<(), Self::Error>;
    fn send_video(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

pub trait RylusReceiver: Iterator<Item = Result<MessageInbound, Self::Error>> {
    type Error: std::error::Error;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Hello roundtrip ----

    #[test]
    fn hello_roundtrip() {
        let hello = Hello {
            protocol_version: PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&hello).unwrap();
        let back: Hello = serde_json::from_str(&json).unwrap();
        assert_eq!(back.protocol_version, PROTOCOL_VERSION);
    }

    // ---- MessageInbound variants roundtrip ----

    #[test]
    fn message_inbound_hello_roundtrip() {
        let msg = MessageInbound::Hello(Hello {
            protocol_version: 1,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageInbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageInbound::Hello(h) => assert_eq!(h.protocol_version, 1),
            other => panic!("Expected Hello, got {:?}", other),
        }
    }

    #[test]
    fn message_inbound_get_capturable_list_roundtrip() {
        let msg = MessageInbound::GetCapturableList;
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageInbound = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, MessageInbound::GetCapturableList));
    }

    #[test]
    fn message_inbound_pause_resume_restart_roundtrip() {
        for msg in [
            MessageInbound::PauseVideo,
            MessageInbound::ResumeVideo,
            MessageInbound::RestartVideo,
            MessageInbound::Heartbeat,
        ] {
            let json = serde_json::to_string(&msg).unwrap();
            let _: MessageInbound = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn message_inbound_buffer_health_roundtrip() {
        let msg = MessageInbound::BufferHealth(BufferHealth {
            buffer_seconds: 1.5,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageInbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageInbound::BufferHealth(bh) => {
                assert!((bh.buffer_seconds - 1.5).abs() < f64::EPSILON);
            }
            other => panic!("Expected BufferHealth, got {:?}", other),
        }
    }

    // ---- MessageOutbound variants roundtrip ----

    #[test]
    fn message_outbound_hello_roundtrip() {
        let msg = MessageOutbound::Hello(Hello {
            protocol_version: 1,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageOutbound::Hello(h) => assert_eq!(h.protocol_version, 1),
            other => panic!("Expected Hello, got {:?}", other),
        }
    }

    #[test]
    fn message_outbound_capturable_list_roundtrip() {
        let msg = MessageOutbound::CapturableList(vec!["Screen 0".into(), "Window A".into()]);
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageOutbound::CapturableList(list) => {
                assert_eq!(list, vec!["Screen 0", "Window A"]);
            }
            other => panic!("Expected CapturableList, got {:?}", other),
        }
    }

    #[test]
    fn message_outbound_new_video_roundtrip() {
        let msg = MessageOutbound::NewVideo;
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, MessageOutbound::NewVideo));
    }

    #[test]
    fn message_outbound_config_ok_roundtrip() {
        let msg = MessageOutbound::ConfigOk;
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, MessageOutbound::ConfigOk));
    }

    #[test]
    fn message_outbound_error_roundtrip() {
        let msg = MessageOutbound::Error("something broke".into());
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageOutbound::Error(s) => assert_eq!(s, "something broke"),
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn message_outbound_config_error_roundtrip() {
        let msg = MessageOutbound::ConfigError("bad config".into());
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageOutbound::ConfigError(s) => assert_eq!(s, "bad config"),
            other => panic!("Expected ConfigError, got {:?}", other),
        }
    }

    // ---- Rect ----

    #[test]
    fn rect_default() {
        let r = Rect::default();
        assert_eq!(r.x, 0.0);
        assert_eq!(r.y, 0.0);
        assert_eq!(r.w, 1.0);
        assert_eq!(r.h, 1.0);
    }

    #[test]
    fn rect_roundtrip() {
        let r = Rect {
            x: 0.1,
            y: 0.2,
            w: 0.5,
            h: 0.6,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: Rect = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    // ---- CustomInputAreas ----

    #[test]
    fn custom_input_areas_default_is_all_none() {
        let c = CustomInputAreas::default();
        assert!(c.mouse.is_none());
        assert!(c.touch.is_none());
        assert!(c.pen.is_none());
    }

    #[test]
    fn custom_input_areas_roundtrip_with_values() {
        let c = CustomInputAreas {
            mouse: Some(Rect {
                x: 0.0,
                y: 0.0,
                w: 0.5,
                h: 0.5,
            }),
            touch: None,
            pen: Some(Rect::default()),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: CustomInputAreas = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn message_outbound_custom_input_areas_roundtrip() {
        let msg = MessageOutbound::CustomInputAreas(CustomInputAreas {
            mouse: Some(Rect {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.4,
            }),
            touch: Some(Rect {
                x: 0.5,
                y: 0.6,
                w: 0.2,
                h: 0.1,
            }),
            pen: None,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageOutbound::CustomInputAreas(c) => {
                assert!(c.mouse.is_some());
                assert!(c.touch.is_some());
                assert!(c.pen.is_none());
            }
            other => panic!("Expected CustomInputAreas, got {:?}", other),
        }
    }

    // ---- PointerType serde ----

    #[test]
    fn pointer_type_serde_names() {
        // PointerType uses custom rename strings
        let json = serde_json::to_string(&PointerType::Mouse).unwrap();
        assert_eq!(json, "\"mouse\"");
        let json = serde_json::to_string(&PointerType::Pen).unwrap();
        assert_eq!(json, "\"pen\"");
        let json = serde_json::to_string(&PointerType::Touch).unwrap();
        assert_eq!(json, "\"touch\"");
        let json = serde_json::to_string(&PointerType::Unknown).unwrap();
        assert_eq!(json, "\"\"");
    }

    #[test]
    fn pointer_type_deserialize() {
        let pt: PointerType = serde_json::from_str("\"mouse\"").unwrap();
        assert!(matches!(pt, PointerType::Mouse));
        let pt: PointerType = serde_json::from_str("\"\"").unwrap();
        assert!(matches!(pt, PointerType::Unknown));
    }

    // ---- PointerEventType serde ----

    #[test]
    fn pointer_event_type_serde_names() {
        let json = serde_json::to_string(&PointerEventType::DOWN).unwrap();
        assert_eq!(json, "\"pointerdown\"");
        let json = serde_json::to_string(&PointerEventType::UP).unwrap();
        assert_eq!(json, "\"pointerup\"");
        let json = serde_json::to_string(&PointerEventType::MOVE).unwrap();
        assert_eq!(json, "\"pointermove\"");
    }

    // ---- KeyboardEventType serde ----

    #[test]
    fn keyboard_event_type_serde_names() {
        let json = serde_json::to_string(&KeyboardEventType::DOWN).unwrap();
        assert_eq!(json, "\"down\"");
        let json = serde_json::to_string(&KeyboardEventType::UP).unwrap();
        assert_eq!(json, "\"up\"");
        let json = serde_json::to_string(&KeyboardEventType::REPEAT).unwrap();
        assert_eq!(json, "\"repeat\"");
    }

    // ---- KeyboardLocation deserialization ----

    #[test]
    fn keyboard_location_deserialize_valid_via_struct() {
        // KeyboardLocation uses a custom deserializer (location_from) that reads a u8,
        // so we must test it via a KeyboardEvent struct, not standalone.
        for (loc_num, expected) in [(0, "STANDARD"), (1, "LEFT"), (2, "RIGHT"), (3, "NUMPAD")] {
            let json = format!(
                r#"{{"event_type":"down","code":"KeyA","key":"a","location":{},"alt":false,"ctrl":false,"shift":false,"meta":false}}"#,
                loc_num
            );
            let ke: KeyboardEvent = serde_json::from_str(&json).unwrap();
            let loc_str = format!("{:?}", ke.location);
            assert_eq!(
                loc_str, expected,
                "location {} should be {}",
                loc_num, expected
            );
        }
    }

    #[test]
    fn keyboard_location_deserialize_invalid_via_struct() {
        let json = r#"{"event_type":"down","code":"KeyA","key":"a","location":4,"alt":false,"ctrl":false,"shift":false,"meta":false}"#;
        let result = serde_json::from_str::<KeyboardEvent>(json);
        assert!(result.is_err());
        let json = r#"{"event_type":"down","code":"KeyA","key":"a","location":255,"alt":false,"ctrl":false,"shift":false,"meta":false}"#;
        let result = serde_json::from_str::<KeyboardEvent>(json);
        assert!(result.is_err());
    }

    // ---- Button bitflags ----

    #[test]
    fn button_from_bits_valid() {
        let b = Button::from_bits(0b0000_0001).unwrap();
        assert_eq!(b, Button::PRIMARY);
        let b = Button::from_bits(0b0000_0011).unwrap();
        assert!(b.contains(Button::PRIMARY));
        assert!(b.contains(Button::SECONDARY));
    }

    #[test]
    fn button_from_bits_invalid() {
        // bit 7 (0b0100_0000) is not defined
        let b = Button::from_bits(0b0100_0000);
        assert!(b.is_none());
    }

    #[test]
    fn button_none_is_zero() {
        assert_eq!(Button::NONE.bits(), 0);
    }

    #[test]
    fn button_eraser_flag() {
        let b = Button::ERASER;
        assert_eq!(b.bits(), 0b0010_0000);
        assert!(!b.contains(Button::PRIMARY));
    }

    // ---- button_from deserializer ----

    #[test]
    fn button_from_deserialize_valid() {
        // button_from is used via PointerEvent; test via JSON struct
        let json = r#"{
            "event_type": "pointerdown",
            "pointer_id": 1,
            "timestamp": 12345,
            "is_primary": true,
            "pointer_type": "mouse",
            "button": 1,
            "buttons": 1,
            "x": 0.5,
            "y": 0.5,
            "pressure": 0.5,
            "tilt_x": 0,
            "tilt_y": 0,
            "twist": 0,
            "width": 0.0,
            "height": 0.0
        }"#;
        let pe: PointerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(pe.button, Button::PRIMARY);
        assert_eq!(pe.buttons, Button::PRIMARY);
    }

    #[test]
    fn button_from_deserialize_invalid_bits() {
        let json = r#"{
            "event_type": "pointerdown",
            "pointer_id": 1,
            "timestamp": 0,
            "is_primary": true,
            "pointer_type": "mouse",
            "button": 128,
            "buttons": 0,
            "x": 0.0,
            "y": 0.0,
            "pressure": 0.0,
            "tilt_x": 0,
            "tilt_y": 0,
            "twist": 0,
            "width": 0.0,
            "height": 0.0
        }"#;
        let result = serde_json::from_str::<PointerEvent>(json);
        assert!(result.is_err());
    }

    // ---- Full PointerEvent roundtrip ----

    #[test]
    fn pointer_event_deserialize() {
        let json = r#"{
            "event_type": "pointermove",
            "pointer_id": 42,
            "timestamp": 99999,
            "is_primary": false,
            "pointer_type": "pen",
            "button": 32,
            "buttons": 33,
            "x": 0.75,
            "y": 0.25,
            "pressure": 0.8,
            "tilt_x": -45,
            "tilt_y": 30,
            "twist": 90,
            "width": 0.01,
            "height": 0.02
        }"#;
        let pe: PointerEvent = serde_json::from_str(json).unwrap();
        assert_eq!(pe.pointer_id, 42);
        assert!(!pe.is_primary);
        assert!(matches!(pe.pointer_type, PointerType::Pen));
        assert_eq!(pe.button, Button::ERASER);
        assert!(pe.buttons.contains(Button::PRIMARY));
        assert!(pe.buttons.contains(Button::ERASER));
        assert_eq!(pe.tilt_x, -45);
        assert_eq!(pe.twist, 90);
        assert!((pe.pressure - 0.8).abs() < f64::EPSILON);
    }

    // ---- Full KeyboardEvent roundtrip ----

    #[test]
    fn keyboard_event_deserialize() {
        let json = r#"{
            "event_type": "down",
            "code": "KeyA",
            "key": "a",
            "location": 0,
            "alt": false,
            "ctrl": true,
            "shift": false,
            "meta": false
        }"#;
        let ke: KeyboardEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ke.event_type, KeyboardEventType::DOWN));
        assert_eq!(ke.code, "KeyA");
        assert_eq!(ke.key, "a");
        assert!(matches!(ke.location, KeyboardLocation::STANDARD));
        assert!(ke.ctrl);
        assert!(!ke.alt);
    }

    #[test]
    fn keyboard_event_numpad_location() {
        let json = r#"{
            "event_type": "down",
            "code": "Numpad5",
            "key": "5",
            "location": 3,
            "alt": false,
            "ctrl": false,
            "shift": false,
            "meta": false
        }"#;
        let ke: KeyboardEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(ke.location, KeyboardLocation::NUMPAD));
    }

    // ---- WheelEvent roundtrip ----

    #[test]
    fn wheel_event_roundtrip() {
        let we = WheelEvent {
            dx: -3,
            dy: 120,
            timestamp: 5000,
        };
        let json = serde_json::to_string(&we).unwrap();
        let back: WheelEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.dx, -3);
        assert_eq!(back.dy, 120);
        assert_eq!(back.timestamp, 5000);
    }

    // ---- ClientConfiguration roundtrip ----

    #[test]
    fn client_configuration_roundtrip() {
        let cc = ClientConfiguration {
            #[cfg(target_os = "linux")]
            uinput_support: true,
            capturable_id: 0,
            capture_cursor: true,
            max_width: 1920,
            max_height: 1080,
            client_name: Some("test-client".into()),
            frame_rate: 30.0,
        };
        let json = serde_json::to_string(&cc).unwrap();
        let back: ClientConfiguration = serde_json::from_str(&json).unwrap();
        assert_eq!(back.capturable_id, 0);
        assert_eq!(back.max_width, 1920);
        assert_eq!(back.max_height, 1080);
        assert_eq!(back.client_name.as_deref(), Some("test-client"));
        assert!((back.frame_rate - 30.0).abs() < f64::EPSILON);
    }

    // ---- MessageInbound with PointerEvent ----

    #[test]
    fn message_inbound_pointer_event_roundtrip() {
        let json = r#"{"PointerEvent":{
            "event_type": "pointerup",
            "pointer_id": 0,
            "timestamp": 100,
            "is_primary": true,
            "pointer_type": "touch",
            "button": 1,
            "buttons": 0,
            "x": 0.0,
            "y": 1.0,
            "pressure": 0.0,
            "tilt_x": 0,
            "tilt_y": 0,
            "twist": 0,
            "width": 0.05,
            "height": 0.05
        }}"#;
        let msg: MessageInbound = serde_json::from_str(json).unwrap();
        match msg {
            MessageInbound::PointerEvent(pe) => {
                assert!(matches!(pe.event_type, PointerEventType::UP));
                assert!(matches!(pe.pointer_type, PointerType::Touch));
                assert_eq!(pe.y, 1.0);
            }
            other => panic!("Expected PointerEvent, got {:?}", other),
        }
    }

    // ---- MessageInbound with WheelEvent ----

    #[test]
    fn message_inbound_wheel_event_roundtrip() {
        let msg = MessageInbound::WheelEvent(WheelEvent {
            dx: 0,
            dy: -1,
            timestamp: 0,
        });
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageInbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageInbound::WheelEvent(we) => assert_eq!(we.dy, -1),
            other => panic!("Expected WheelEvent, got {:?}", other),
        }
    }

    // ---- Edge case: empty capturable list ----

    #[test]
    fn message_outbound_empty_capturable_list() {
        let msg = MessageOutbound::CapturableList(vec![]);
        let json = serde_json::to_string(&msg).unwrap();
        let back: MessageOutbound = serde_json::from_str(&json).unwrap();
        match back {
            MessageOutbound::CapturableList(list) => assert!(list.is_empty()),
            other => panic!("Expected CapturableList, got {:?}", other),
        }
    }

    // ---- Malformed JSON ----

    #[test]
    fn message_inbound_malformed_json() {
        let result = serde_json::from_str::<MessageInbound>("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn message_inbound_unknown_variant() {
        let result = serde_json::from_str::<MessageInbound>(r#""NonExistentVariant""#);
        assert!(result.is_err());
    }

    #[test]
    fn message_outbound_malformed_json() {
        let result = serde_json::from_str::<MessageOutbound>("{\"Hello\": {}}");
        assert!(result.is_err()); // missing protocol_version
    }
}
