use windows::Win32::Foundation::{HANDLE, HWND, POINT};
use windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_DEFAULT, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::Input::Pointer::{
    InitializeTouchInjection, InjectSyntheticPointerInput,
    POINTER_CHANGE_FIRSTBUTTON_DOWN, POINTER_CHANGE_NONE, POINTER_CHANGE_SECONDBUTTON_DOWN,
    POINTER_CHANGE_THIRDBUTTON_DOWN, POINTER_FLAG_CANCELED, POINTER_FLAG_DOWN,
    POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE, POINTER_FLAG_PRIMARY, POINTER_FLAG_UP,
    POINTER_FLAG_UPDATE, POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO,
    TOUCH_FEEDBACK_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetCursorPos, PEN_FLAG_NONE, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X,
    PEN_MASK_TILT_Y, PT_PEN, PT_TOUCH, TOUCH_FLAG_NONE, TOUCH_MASK_PRESSURE,
};

use tracing::warn;

use crate::device::{InputDevice, InputDeviceType};
use crate::enigo_device::EnigoDevice;
use rylus_core::protocol::{
    Button, KeyboardEvent, PointerEvent, PointerEventType, PointerType, WheelEvent,
};

use rylus_core::{Capturable, Geometry};

pub struct WindowsInput {
    capturable: Box<dyn Capturable>,
    enigo_device: EnigoDevice,
    pointer_device_handle: HSYNTHETICPOINTERDEVICE,
    touch_device_handle: HSYNTHETICPOINTERDEVICE,
    multitouch_map: std::collections::HashMap<i64, POINTER_TYPE_INFO>,
}

impl WindowsInput {
    pub fn new(capturable: Box<dyn Capturable>) -> Self {
        // SAFETY: InitializeTouchInjection and CreateSyntheticPointerDevice are Windows API
        // calls that initialize system input injection resources. The returned device handles
        // are stored and destroyed in Drop.
        unsafe {
            let _ = InitializeTouchInjection(5, TOUCH_FEEDBACK_DEFAULT);
            Self {
                capturable: capturable.clone(),
                enigo_device: EnigoDevice::new(capturable),
                pointer_device_handle: CreateSyntheticPointerDevice(
                    PT_PEN,
                    1,
                    POINTER_FEEDBACK_DEFAULT,
                )
                .expect("Failed to create pen synthetic pointer device"),
                touch_device_handle: CreateSyntheticPointerDevice(
                    PT_TOUCH,
                    5,
                    POINTER_FEEDBACK_DEFAULT,
                )
                .expect("Failed to create touch synthetic pointer device"),
                multitouch_map: std::collections::HashMap::new(),
            }
        }
    }
}

impl InputDevice for WindowsInput {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        // SAFETY: mouse_event is a Windows API function that synthesizes mouse input;
        // all parameters are plain integers with no pointer aliasing concerns.
        unsafe { mouse_event(MOUSEEVENTF_WHEEL, 0, 0, event.dy as i32, 0) };
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        let Geometry::VirtualScreen(offset_x, offset_y, width, height, left, top) =
            (match self.capturable.geometry() {
                Ok(g) => g,
                Err(err) => {
                    warn!("Failed to get capturable geometry: {}", err);
                    return;
                }
            })
        else {
            unreachable!()
        };

        let (x, y) = (
            (event.x * width as f64) as i32 + offset_x,
            (event.y * height as f64) as i32 + offset_y,
        );
        let mut pointer_flags = match event.event_type {
            PointerEventType::DOWN => {
                POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN
            }
            PointerEventType::MOVE | PointerEventType::OVER | PointerEventType::ENTER => {
                POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE
            }
            PointerEventType::UP => POINTER_FLAG_UP,
            PointerEventType::CANCEL | PointerEventType::LEAVE | PointerEventType::OUT => {
                POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE | POINTER_FLAG_CANCELED
            }
        };
        let button_change_type = match event.buttons {
            Button::PRIMARY => {
                pointer_flags |= POINTER_FLAG_INCONTACT;
                POINTER_CHANGE_FIRSTBUTTON_DOWN
            }
            Button::SECONDARY => POINTER_CHANGE_SECONDBUTTON_DOWN,
            Button::AUXILARY => POINTER_CHANGE_THIRDBUTTON_DOWN,
            Button::NONE => POINTER_CHANGE_NONE,
            _ => POINTER_CHANGE_NONE,
        };
        if event.is_primary {
            pointer_flags |= POINTER_FLAG_PRIMARY;
        }
        match event.pointer_type {
            PointerType::Pen => {
                // SAFETY: POINTER_TYPE_INFO's union field is C-repr and all-zeros is a
                // valid bit pattern for the union variants. InjectSyntheticPointerInput
                // is called with a valid device handle and a properly initialized struct.
                unsafe {
                    let pen_info = POINTER_PEN_INFO {
                        pointerInfo: POINTER_INFO {
                            pointerType: PT_PEN,
                            pointerId: event.pointer_id as u32,
                            frameId: 0,
                            pointerFlags: pointer_flags,
                            sourceDevice: HANDLE::default(),
                            hwndTarget: HWND::default(),
                            ptPixelLocation: POINT { x, y },
                            ptHimetricLocation: POINT { x: 0, y: 0 },
                            ptPixelLocationRaw: POINT { x, y },
                            ptHimetricLocationRaw: POINT { x: 0, y: 0 },
                            dwTime: 0,
                            historyCount: 1,
                            InputData: 0,
                            dwKeyStates: 0,
                            PerformanceCount: 0,
                            ButtonChangeType: button_change_type,
                        },
                        penFlags: PEN_FLAG_NONE,
                        penMask: PEN_MASK_PRESSURE
                            | PEN_MASK_ROTATION
                            | PEN_MASK_TILT_X
                            | PEN_MASK_TILT_Y,
                        pressure: (event.pressure * 1024f64) as u32,
                        rotation: event.twist as u32,
                        tiltX: event.tilt_x,
                        tiltY: event.tilt_y,
                    };
                    let pointer_type_info = POINTER_TYPE_INFO {
                        r#type: PT_PEN,
                        Anonymous: POINTER_TYPE_INFO_0 { penInfo: pen_info },
                    };
                    let _ = InjectSyntheticPointerInput(
                        self.pointer_device_handle,
                        &[pointer_type_info],
                    );
                }
            }
            PointerType::Touch => {
                // SAFETY: POINTER_TYPE_INFO and POINTER_TOUCH_INFO are C-repr structs where
                // all-zeros is a valid bit pattern. InjectSyntheticPointerInput is called
                // with a valid device handle and properly initialized structs.
                unsafe {
                    let mut pointer_touch_info: POINTER_TOUCH_INFO = std::mem::zeroed();
                    pointer_touch_info.pointerInfo = std::mem::zeroed();
                    pointer_touch_info.pointerInfo.pointerType = PT_TOUCH;
                    pointer_touch_info.pointerInfo.pointerFlags = pointer_flags;
                    pointer_touch_info.pointerInfo.pointerId = event.pointer_id as u32;
                    pointer_touch_info.pointerInfo.ptPixelLocation = POINT { x, y };
                    pointer_touch_info.touchFlags = TOUCH_FLAG_NONE;
                    pointer_touch_info.touchMask = TOUCH_MASK_PRESSURE;
                    pointer_touch_info.pressure = (event.pressure * 1024f64) as u32;

                    pointer_touch_info.pointerInfo.ButtonChangeType = button_change_type;

                    let pointer_type_info = POINTER_TYPE_INFO {
                        r#type: PT_TOUCH,
                        Anonymous: POINTER_TYPE_INFO_0 {
                            touchInfo: pointer_touch_info,
                        },
                    };
                    self.multitouch_map
                        .insert(event.pointer_id, pointer_type_info);

                    let pointer_type_info_vec: Vec<POINTER_TYPE_INFO> =
                        self.multitouch_map.values().copied().collect();
                    let _ = InjectSyntheticPointerInput(
                        self.touch_device_handle,
                        &pointer_type_info_vec,
                    );

                    match event.event_type {
                        PointerEventType::DOWN
                        | PointerEventType::MOVE
                        | PointerEventType::OVER
                        | PointerEventType::ENTER => {}

                        PointerEventType::UP
                        | PointerEventType::CANCEL
                        | PointerEventType::LEAVE
                        | PointerEventType::OUT => {
                            self.multitouch_map.remove(&event.pointer_id);
                        }
                    }
                }
            }
            PointerType::Mouse => {
                let mut dw_flags = MOUSE_EVENT_FLAGS(0);

                let (screen_x, screen_y) = (
                    (event.x * width as f64) as i32 + left,
                    (event.y * height as f64) as i32 + top,
                );

                match event.event_type {
                    PointerEventType::DOWN => match event.buttons {
                        Button::PRIMARY => {
                            dw_flags |= MOUSEEVENTF_LEFTDOWN;
                        }
                        Button::SECONDARY => {
                            dw_flags |= MOUSEEVENTF_RIGHTDOWN;
                        }
                        Button::AUXILARY => {
                            dw_flags |= MOUSEEVENTF_MIDDLEDOWN;
                        }
                        _ => {}
                    },
                    PointerEventType::MOVE | PointerEventType::OVER | PointerEventType::ENTER => {
                        // SAFETY: SetCursorPos is a Windows API that moves the cursor;
                        // screen_x/screen_y are valid screen coordinates.
                        unsafe {
                            let _ = SetCursorPos(screen_x, screen_y);
                        }
                    }
                    PointerEventType::UP => match event.button {
                        Button::PRIMARY => {
                            dw_flags |= MOUSEEVENTF_LEFTUP;
                        }
                        Button::SECONDARY => {
                            dw_flags |= MOUSEEVENTF_RIGHTUP;
                        }
                        Button::AUXILARY => {
                            dw_flags |= MOUSEEVENTF_MIDDLEUP;
                        }
                        _ => {}
                    },
                    PointerEventType::CANCEL | PointerEventType::LEAVE | PointerEventType::OUT => {
                        dw_flags |= MOUSEEVENTF_LEFTUP;
                    }
                }
                // SAFETY: mouse_event is a Windows API that synthesizes mouse input;
                // all parameters are plain integers.
                unsafe { mouse_event(dw_flags, 0, 0, 0, 0) };
            }
            PointerType::Unknown => {
                warn!("Received Unknown pointer type, ignoring event.");
                return;
            }
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        self.enigo_device.send_keyboard_event(event);
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable;
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::WindowsInput
    }
}

impl Drop for WindowsInput {
    fn drop(&mut self) {
        // SAFETY: pointer_device_handle and touch_device_handle are valid handles obtained
        // from CreateSyntheticPointerDevice in new() and are destroyed exactly once here.
        unsafe {
            DestroySyntheticPointerDevice(self.pointer_device_handle);
            DestroySyntheticPointerDevice(self.touch_device_handle);
        }
    }
}
