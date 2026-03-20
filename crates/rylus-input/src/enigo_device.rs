use enigo::{
    Axis, Button as EnigoButton, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};

use tracing::warn;

use crate::device::{InputDevice, InputDeviceType};
use rylus_core::protocol::{
    Button, KeyboardEvent, KeyboardEventType, PointerEvent, WheelEvent,
};

use rylus_core::{Capturable, Geometry};

pub struct EnigoDevice {
    enigo: Enigo,
    capturable: Box<dyn Capturable>,
}

impl EnigoDevice {
    pub fn new(capturable: Box<dyn Capturable>) -> Self {
        let enigo = Enigo::new(&Settings::default()).expect("Failed to create Enigo instance");
        Self { enigo, capturable }
    }
}

impl InputDevice for EnigoDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        if event.dy != 0 {
            if let Err(e) = self.enigo.scroll(-event.dy, Axis::Vertical) {
                warn!("Failed to scroll vertically: {}", e);
            }
        }
        if event.dx != 0 {
            if let Err(e) = self.enigo.scroll(event.dx, Axis::Horizontal) {
                warn!("Failed to scroll horizontally: {}", e);
            }
        }
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        // Enigo only supports a single pointer; non-primary events are intentionally ignored.
        if !event.is_primary {
            return;
        }
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        let geometry = match self.capturable.geometry() {
            Ok(g) => g,
            Err(e) => {
                warn!("Failed to get window geometry, sending no input ({})", e);
                return;
            }
        };
        let (x_rel, y_rel, width_rel, height_rel) = match geometry {
            Geometry::Relative(x, y, width, height) => (x, y, width, height),
            #[cfg(target_os = "windows")]
            Geometry::VirtualScreen(..) => {
                warn!("VirtualScreen geometry not supported by EnigoDevice, sending no input");
                return;
            }
        };

        let (width, height) = match self.enigo.main_display() {
            Ok((w, h)) => (w as f64, h as f64),
            Err(e) => {
                warn!("Failed to get display size: {}", e);
                return;
            }
        };

        let px = ((event.x * width_rel + x_rel) * width).clamp(0.0, width) as i32;
        let py = ((event.y * height_rel + y_rel) * height).clamp(0.0, height) as i32;
        if let Err(e) = self.enigo.move_mouse(px, py, Coordinate::Abs) {
            warn!("Could not move mouse: {}", e);
        }
        let pressed = event.buttons.contains(event.button);
        match event.button {
            Button::PRIMARY => {
                let _ = self
                    .enigo
                    .button(EnigoButton::Left, if pressed { Direction::Press } else { Direction::Release });
            }
            Button::AUXILARY => {
                let _ = self
                    .enigo
                    .button(EnigoButton::Middle, if pressed { Direction::Press } else { Direction::Release });
            }
            Button::SECONDARY => {
                let _ = self
                    .enigo
                    .button(EnigoButton::Right, if pressed { Direction::Press } else { Direction::Release });
            }
            _ => (),
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        let direction = match event.event_type {
            KeyboardEventType::UP => Direction::Release,
            KeyboardEventType::DOWN => Direction::Press,
            // enigo doesn't have a repeat concept; just re-press
            KeyboardEventType::REPEAT => Direction::Press,
        };

        fn map_key(code: &str) -> Option<Key> {
            match code {
                "Escape" => Some(Key::Escape),
                "Enter" => Some(Key::Return),
                "Backspace" => Some(Key::Backspace),
                "Tab" => Some(Key::Tab),
                "Space" => Some(Key::Space),
                "CapsLock" => Some(Key::CapsLock),
                "F1" => Some(Key::F1),
                "F2" => Some(Key::F2),
                "F3" => Some(Key::F3),
                "F4" => Some(Key::F4),
                "F5" => Some(Key::F5),
                "F6" => Some(Key::F6),
                "F7" => Some(Key::F7),
                "F8" => Some(Key::F8),
                "F9" => Some(Key::F9),
                "F10" => Some(Key::F10),
                "F11" => Some(Key::F11),
                "F12" => Some(Key::F12),
                "F13" => Some(Key::F13),
                "F14" => Some(Key::F14),
                "F15" => Some(Key::F15),
                "F16" => Some(Key::F16),
                "F17" => Some(Key::F17),
                "F18" => Some(Key::F18),
                "F19" => Some(Key::F19),
                "F20" => Some(Key::F20),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "F21" => Some(Key::F21),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "F22" => Some(Key::F22),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "F23" => Some(Key::F23),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "F24" => Some(Key::F24),
                "Home" => Some(Key::Home),
                "ArrowUp" => Some(Key::UpArrow),
                "PageUp" => Some(Key::PageUp),
                "ArrowLeft" => Some(Key::LeftArrow),
                "ArrowRight" => Some(Key::RightArrow),
                "End" => Some(Key::End),
                "ArrowDown" => Some(Key::DownArrow),
                "PageDown" => Some(Key::PageDown),
                "Delete" => Some(Key::Delete),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "Insert" => Some(Key::Insert),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "PrintScreen" => Some(Key::PrintScr),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "Pause" => Some(Key::Pause),
                #[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
                "NumLock" => Some(Key::Numlock),
                #[cfg(all(unix, not(target_os = "macos")))]
                "ScrollLock" => Some(Key::ScrollLock),
                "Numpad0" => Some(Key::Numpad0),
                "Numpad1" => Some(Key::Numpad1),
                "Numpad2" => Some(Key::Numpad2),
                "Numpad3" => Some(Key::Numpad3),
                "Numpad4" => Some(Key::Numpad4),
                "Numpad5" => Some(Key::Numpad5),
                "Numpad6" => Some(Key::Numpad6),
                "Numpad7" => Some(Key::Numpad7),
                "Numpad8" => Some(Key::Numpad8),
                "Numpad9" => Some(Key::Numpad9),
                "NumpadAdd" => Some(Key::Add),
                "NumpadSubtract" => Some(Key::Subtract),
                "NumpadMultiply" => Some(Key::Multiply),
                "NumpadDivide" => Some(Key::Divide),
                "NumpadDecimal" => Some(Key::Decimal),
                "NumpadEnter" => Some(Key::Return),
                "ControlLeft" => Some(Key::LControl),
                "ControlRight" => Some(Key::RControl),
                "AltLeft" => Some(Key::Alt),
                "AltRight" => Some(Key::Alt),
                "MetaLeft" | "MetaRight" => Some(Key::Meta),
                "ShiftLeft" => Some(Key::LShift),
                "ShiftRight" => Some(Key::RShift),
                "VolumeMute" | "AudioVolumeMute" => Some(Key::VolumeMute),
                "VolumeDown" | "AudioVolumeDown" => Some(Key::VolumeDown),
                "VolumeUp" | "AudioVolumeUp" => Some(Key::VolumeUp),
                "MediaTrackNext" => Some(Key::MediaNextTrack),
                "MediaPlayPause" => Some(Key::MediaPlayPause),
                "MediaTrackPrevious" => Some(Key::MediaPrevTrack),
                _ => None,
            }
        }

        let key = map_key(&event.code);
        match key {
            Some(k) => {
                // Send modifier keys if flagged (matching autopilot behavior)
                if event.ctrl {
                    let _ = self.enigo.key(Key::Control, direction);
                }
                if event.alt {
                    let _ = self.enigo.key(Key::Alt, direction);
                }
                if event.meta {
                    let _ = self.enigo.key(Key::Meta, direction);
                }
                if event.shift {
                    let _ = self.enigo.key(Key::Shift, direction);
                }
                let _ = self.enigo.key(k, direction);
            }
            None => {
                // For unmapped keys, try entering as unicode characters
                if let KeyboardEventType::DOWN = event.event_type {
                    for c in event.key.chars() {
                        let _ = self.enigo.key(Key::Unicode(c), direction);
                    }
                }
            }
        }
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable;
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::EnigoDevice
    }
}
