use rylus_capture::Capturable;
use rylus_core::protocol::{KeyboardEvent, PointerEvent, WheelEvent};

#[derive(PartialEq, Eq)]
pub enum InputDeviceType {
    EnigoDevice,
    UInputDevice,
    #[cfg(target_os = "windows")]
    WindowsInput,
}

pub trait InputDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent);
    fn send_pointer_event(&mut self, event: &PointerEvent);
    fn send_keyboard_event(&mut self, event: &KeyboardEvent);
    fn set_capturable(&mut self, capturable: Box<dyn Capturable>);
    fn device_type(&self) -> InputDeviceType;
}
