use std::cmp::Ordering;
use std::os::raw::c_int;
use std::time::{Duration, Instant};

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    PropType, RelativeAxisCode, UinputAbsSetup,
};

#[cfg(feature = "x11")]
use rylus_capture::x11::X11Context;
use rylus_core::{Capturable, Geometry};

use crate::device::{InputDevice, InputDeviceType};
use rylus_core::error::CError;
use rylus_core::protocol::{
    Button, KeyboardEvent, KeyboardEventType, KeyboardLocation, PointerEvent, PointerEventType,
    PointerType, Rect, WheelEvent,
};

use tracing::{debug, warn};

struct MultiTouch {
    id: i64,
}

const ABS_MAXVAL: i32 = 65535;

fn device_input_id() -> InputId {
    InputId::new(BusType::BUS_VIRTUAL, 0x1701, 0x1701, 0x0001)
}

fn map_io_error(e: std::io::Error, context: &str) -> CError {
    let code = if e.kind() == std::io::ErrorKind::PermissionDenied
        || e.kind() == std::io::ErrorKind::NotFound
    {
        101 // UInputNotAccessible
    } else {
        1 // GenericError
    };
    CError::with_code(code, &format!("{}: {}", context, e))
}

/// Create a single InputEvent from raw type/code/value.
fn ev(typ: EventType, code: c_int, value: c_int) -> InputEvent {
    InputEvent::new(typ.0, code as u16, value)
}

/// Emit a batch of events to a device. `emit()` auto-appends SYN_REPORT.
fn emit_events(device: &mut VirtualDevice, events: &[InputEvent]) {
    if let Err(e) = device.emit(events) {
        warn!("Error writing uinput events: {}", e);
    }
}

fn create_keyboard(name: &str) -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    // KEY_ESC (1) through KEY_MICMUTE (248)
    for code in 1u16..=248 {
        keys.insert(KeyCode::new(code));
    }
    VirtualDevice::builder()?
        .name(name)
        .input_id(device_input_id())
        .with_keys(&keys)?
        .build()
}

fn create_mouse(name: &str) -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::BTN_LEFT);
    keys.insert(KeyCode::BTN_RIGHT);
    keys.insert(KeyCode::BTN_MIDDLE);

    let mut rel_axes = AttributeSet::<RelativeAxisCode>::new();
    rel_axes.insert(RelativeAxisCode::REL_WHEEL);
    rel_axes.insert(RelativeAxisCode::REL_HWHEEL);
    rel_axes.insert(RelativeAxisCode::REL_WHEEL_HI_RES);
    rel_axes.insert(RelativeAxisCode::REL_HWHEEL_HI_RES);

    let mut props = AttributeSet::<PropType>::new();
    props.insert(PropType::DIRECT);

    VirtualDevice::builder()?
        .name(name)
        .input_id(device_input_id())
        .with_keys(&keys)?
        .with_relative_axes(&rel_axes)?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_X,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 0),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_Y,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 0),
        ))?
        .with_properties(&props)?
        .build()
}

fn create_stylus(name: &str) -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::BTN_TOOL_PEN);
    keys.insert(KeyCode::BTN_TOOL_RUBBER);
    keys.insert(KeyCode::BTN_TOUCH);

    let mut props = AttributeSet::<PropType>::new();
    props.insert(PropType::DIRECT);

    VirtualDevice::builder()?
        .name(name)
        .input_id(device_input_id())
        .with_keys(&keys)?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_X,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_Y,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_PRESSURE,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_TILT_X,
            AbsInfo::new(0, -90, 90, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_TILT_Y,
            AbsInfo::new(0, -90, 90, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_DISTANCE,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 12),
        ))?
        .with_properties(&props)?
        .build()
}

fn create_touch(name: &str) -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::BTN_TOUCH);
    keys.insert(KeyCode::BTN_TOOL_FINGER);
    keys.insert(KeyCode::BTN_TOOL_DOUBLETAP);
    keys.insert(KeyCode::BTN_TOOL_TRIPLETAP);
    keys.insert(KeyCode::BTN_TOOL_QUADTAP);
    keys.insert(KeyCode::BTN_TOOL_QUINTTAP);

    let mut props = AttributeSet::<PropType>::new();
    props.insert(PropType::DIRECT);

    VirtualDevice::builder()?
        .name(name)
        .input_id(device_input_id())
        .with_keys(&keys)?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_X,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 200),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_Y,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 200),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_SLOT,
            AbsInfo::new(0, 0, 4, 0, 0, 0),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_TRACKING_ID,
            AbsInfo::new(0, 0, 4, 0, 0, 0),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_POSITION_X,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 200),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_POSITION_Y,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 200),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_PRESSURE,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 0),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_TOUCH_MAJOR,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_TOUCH_MINOR,
            AbsInfo::new(0, 0, ABS_MAXVAL, 0, 0, 12),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_MT_ORIENTATION,
            AbsInfo::new(0, 0, 1, 0, 0, 0),
        ))?
        .with_properties(&props)?
        .build()
}

pub struct UInputDevice {
    keyboard: VirtualDevice,
    stylus: VirtualDevice,
    mouse: VirtualDevice,
    touch: VirtualDevice,
    // 5 slots (0..=4) to match the ABS_MT_SLOT range declared in
    // create_touch() below. evdev has no BTN_TOOL_* code past QUINTTAP (5
    // fingers), so 5 is the real ceiling regardless of array size.
    touches: [Option<MultiTouch>; 5],
    tool_pen_active: bool,
    pen_touching: bool,
    last_pen_event: Instant,
    capturable: Box<dyn Capturable>,
    geometry: Rect,
    #[cfg(feature = "x11")]
    name_mouse_device: String,
    #[cfg(feature = "x11")]
    name_stylus_device: String,
    #[cfg(feature = "x11")]
    name_touch_device: String,
    #[cfg(feature = "x11")]
    num_mouse_mapping_tries: usize,
    #[cfg(feature = "x11")]
    num_stylus_mapping_tries: usize,
    #[cfg(feature = "x11")]
    num_touch_mapping_tries: usize,
    #[cfg(feature = "x11")]
    x11ctx: Option<X11Context>,
}

impl UInputDevice {
    #[allow(clippy::result_large_err)]
    pub fn new(capturable: Box<dyn Capturable>, id: &Option<String>) -> Result<Self, CError> {
        let suffix = id.as_ref().map_or(String::new(), |id| format!(" - {}", id));

        let name_stylus = format!("Rylus Stylus{}", suffix);
        let name_mouse = format!("Rylus Mouse{}", suffix);
        let name_touch = format!("Rylus Touch{}", suffix);
        let name_keyboard = format!("Rylus Keyboard{}", suffix);

        let stylus = create_stylus(&name_stylus)
            .map_err(|e| map_io_error(e, "error creating stylus device"))?;
        let mouse = create_mouse(&name_mouse)
            .map_err(|e| map_io_error(e, "error creating mouse device"))?;
        let touch = create_touch(&name_touch)
            .map_err(|e| map_io_error(e, "error creating touch device"))?;
        // Suppress unused variable warnings when x11 feature is disabled
        #[cfg(not(feature = "x11"))]
        let _ = (&name_mouse, &name_touch, &name_stylus);
        let keyboard = create_keyboard(&name_keyboard)
            .map_err(|e| map_io_error(e, "error creating keyboard device"))?;

        Ok(Self {
            keyboard,
            stylus,
            mouse,
            touch,
            touches: Default::default(),
            tool_pen_active: false,
            pen_touching: false,
            last_pen_event: Instant::now(),
            capturable,
            geometry: Rect::default(),
            #[cfg(feature = "x11")]
            name_mouse_device: name_mouse,
            #[cfg(feature = "x11")]
            name_touch_device: name_touch,
            #[cfg(feature = "x11")]
            name_stylus_device: name_stylus,
            #[cfg(feature = "x11")]
            num_mouse_mapping_tries: 0,
            #[cfg(feature = "x11")]
            num_stylus_mapping_tries: 0,
            #[cfg(feature = "x11")]
            num_touch_mapping_tries: 0,
            #[cfg(feature = "x11")]
            x11ctx: X11Context::new(),
        })
    }

    fn transform_x(&self, x: f64) -> i32 {
        compute_transform_x(x, self.geometry.x, self.geometry.w)
    }

    fn transform_y(&self, y: f64) -> i32 {
        compute_transform_y(y, self.geometry.y, self.geometry.h)
    }

    fn transform_pressure(&self, p: f64) -> i32 {
        compute_transform_pressure(p)
    }

    fn transform_touch_size(&self, s: f64) -> i32 {
        compute_transform_touch_size(s)
    }

    fn find_slot(&self, id: i64) -> Option<usize> {
        self.touches
            .iter()
            .enumerate()
            .find_map(|(slot, mt)| match mt {
                Some(mt) => {
                    if mt.id == id {
                        Some(slot)
                    } else {
                        None
                    }
                }
                _ => None,
            })
    }

    /// Clears a stuck `BTN_TOOL_PEN` left asserted by browsers that report pen-hover
    /// `pointermove`/`pointerover` but never fire a terminal "left proximity" event.
    /// Time-gated by `last_pen_event`, so it's a no-op on most calls.
    fn clear_stale_pen_hover(&mut self) {
        if self.tool_pen_active
            && !self.pen_touching
            && (Instant::now() - self.last_pen_event) > Duration::from_millis(50)
        {
            self.tool_pen_active = false;
            emit_events(
                &mut self.stylus,
                &[
                    ev(EventType::KEY, EC_KEY_TOUCH, 0),
                    ev(EventType::KEY, EC_KEY_TOOL_PEN, 0),
                    ev(EventType::KEY, EC_KEY_TOOL_RUBBER, 0),
                    ev(EventType::ABSOLUTE, EC_ABSOLUTE_PRESSURE, 0),
                ],
            );
        }
    }
}

// Event Codes (matching Linux input-event-codes.h)
const EC_KEY_MOUSE_LEFT: c_int = 0x110;
const EC_KEY_MOUSE_RIGHT: c_int = 0x111;
const EC_KEY_MOUSE_MIDDLE: c_int = 0x112;
const EC_KEY_TOOL_PEN: c_int = 0x140;
const EC_KEY_TOOL_RUBBER: c_int = 0x141;
const EC_KEY_TOUCH: c_int = 0x14a;
const EC_KEY_TOOL_FINGER: c_int = 0x145;
const EC_KEY_TOOL_DOUBLETAP: c_int = 0x14d;
const EC_KEY_TOOL_TRIPLETAP: c_int = 0x14e;
const EC_KEY_TOOL_QUADTAP: c_int = 0x14f;
const EC_KEY_TOOL_QUINTTAP: c_int = 0x148;

/// Maps a live count of concurrently-active touches (1..=5) to the evdev
/// BTN_TOOL_* key that represents it. evdev has no tool code past
/// QUINTTAP, so counts above 5 saturate at QUINTTAP (this ceiling is
/// enforced upstream: `UInputDevice.touches` only has 5 slots).
fn tool_key_for_touch_count(count: usize) -> c_int {
    match count {
        1 => EC_KEY_TOOL_FINGER,
        2 => EC_KEY_TOOL_DOUBLETAP,
        3 => EC_KEY_TOOL_TRIPLETAP,
        4 => EC_KEY_TOOL_QUADTAP,
        _ => EC_KEY_TOOL_QUINTTAP,
    }
}

const EC_REL_HWHEEL: c_int = 0x06;
const EC_REL_WHEEL: c_int = 0x08;
const EC_REL_WHEEL_HI_RES: c_int = 0x0b;
const EC_REL_HWHEEL_HI_RES: c_int = 0x0c;

const EC_ABSOLUTE_X: c_int = 0x00;
const EC_ABSOLUTE_Y: c_int = 0x01;
const EC_ABSOLUTE_PRESSURE: c_int = 0x18;
const EC_ABSOLUTE_TILT_X: c_int = 0x1a;
const EC_ABSOLUTE_TILT_Y: c_int = 0x1b;
const EC_ABSOLUTE_DISTANCE: c_int = 0x19;
const EC_ABS_MT_SLOT: c_int = 0x2f;
const EC_ABS_MT_TOUCH_MAJOR: c_int = 0x30;
const EC_ABS_MT_TOUCH_MINOR: c_int = 0x31;
const EC_ABS_MT_ORIENTATION: c_int = 0x34;
const EC_ABS_MT_POSITION_X: c_int = 0x35;
const EC_ABS_MT_POSITION_Y: c_int = 0x36;
const EC_ABS_MT_TRACKING_ID: c_int = 0x39;
const EC_ABS_MT_PRESSURE: c_int = 0x3a;

// This is chosen somewhat arbitrarily
// describes maximum value for ABS_X, ABS_Y, ABS_...
// This corresponds to PointerEvent values of 1.0
const ABS_MAX: f64 = 65535.0;

// This specifies how many times it should be attempted to map the input devices created via uinput
// to the entire screen and not only a single monitor. Actually this is a workaround because
// apparently it is impossible to set the correct mapping in a sane way. The reason is that X needs
// some time to register new input devices, which makes it impossible to configure them right after
// creation as the devices won't be available for configuration at that time. This means one has to
// wait an unspecified amount of time until the devices show up. But just sleeping for example 3
// seconds does not solve the issue either because the input device for the stylus does not show up
// if there has not been any input. As a matter of fact things are even more compilcated as for
// some reason the stylus device created via uinput creates two devices for X. One can not be
// mapped to the screen (this is the device that shows up with out the need to send actual inputs
// via uinput) and another one that can be mapped to the screen. But this is the device that
// requires sending inputs via uinput first other wise it does not show up. This is why this crude
// method of just setting the mapping forcefully on the first MAX_SCREEN_MAPPING_TRIES input events
// has been choosen. If anyone knows a better solution: PLEASE FIX THIS!
#[cfg(feature = "x11")]
const MAX_SCREEN_MAPPING_TRIES: usize = 100;

impl InputDevice for UInputDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }

        fn direction(d: i32) -> i32 {
            match d.cmp(&0) {
                Ordering::Equal => 0,
                Ordering::Less => -1,
                Ordering::Greater => 1,
            }
        }

        emit_events(
            &mut self.mouse,
            &[
                ev(EventType::RELATIVE, EC_REL_WHEEL, direction(event.dy)),
                ev(EventType::RELATIVE, EC_REL_HWHEEL, direction(event.dx)),
                ev(EventType::RELATIVE, EC_REL_WHEEL_HI_RES, event.dy),
                ev(EventType::RELATIVE, EC_REL_HWHEEL_HI_RES, event.dx),
            ],
        );
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
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
        let (x, y, width, height) = match geometry {
            Geometry::Relative(x, y, width, height) => (x, y, width, height),
        };
        self.geometry.x = x;
        self.geometry.y = y;
        self.geometry.w = width;
        self.geometry.h = height;

        // Workaround for browsers that send events when the pen is hovering but do not
        // send an event when the pen leaves the hovering range. This must run for every
        // incoming pointer event regardless of type — not just Touch — otherwise a
        // touch-disabled session (a common drawing-workflow configuration) never clears
        // a stuck BTN_TOOL_PEN, since no Touch event would ever arrive to trigger it.
        // Cheap: it's already time-gated by last_pen_event.
        self.clear_stale_pen_hover();

        match event.pointer_type {
            PointerType::Touch => {
                #[cfg(feature = "x11")]
                if self.num_touch_mapping_tries < MAX_SCREEN_MAPPING_TRIES {
                    if let Some(x11ctx) = &mut self.x11ctx {
                        if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
                            if session_type != "wayland" {
                                x11ctx.map_input_device_to_entire_screen(
                                    &self.name_touch_device,
                                    false,
                                );
                            }
                        } else {
                            x11ctx
                                .map_input_device_to_entire_screen(&self.name_touch_device, false);
                        }
                    }
                    self.num_touch_mapping_tries += 1;
                }

                match event.event_type {
                    PointerEventType::DOWN
                    | PointerEventType::MOVE
                    | PointerEventType::OVER
                    | PointerEventType::ENTER => {
                        let slot: usize;
                        if let Some(s) = self.find_slot(event.pointer_id) {
                            slot = s;
                        } else if let Some(s) =
                            self.touches
                                .iter()
                                .enumerate()
                                .find_map(|(slot, mt)| match mt {
                                    None => Some(slot),
                                    Some(_) => None,
                                })
                        {
                            slot = s;
                            self.touches[slot] = Some(MultiTouch {
                                id: event.pointer_id,
                            })
                        } else {
                            return;
                        };

                        let mut events = vec![
                            ev(EventType::ABSOLUTE, EC_ABS_MT_SLOT, slot as i32),
                            ev(EventType::ABSOLUTE, EC_ABS_MT_TRACKING_ID, slot as i32),
                        ];

                        if let PointerEventType::DOWN = event.event_type {
                            events.push(ev(EventType::KEY, EC_KEY_TOUCH, 1));
                            // Derive the BTN_TOOL_* transition from the live
                            // count of concurrently-active touches, not the
                            // slot index — slots are recycled by
                            // first-free-slot, so slot number stops tracking
                            // finger count as soon as touches release out of
                            // FIFO order.
                            let active_count =
                                self.touches.iter().filter(|t| t.is_some()).count();
                            if active_count > 1 {
                                events.push(ev(
                                    EventType::KEY,
                                    tool_key_for_touch_count(active_count - 1),
                                    0,
                                ));
                            }
                            events.push(ev(
                                EventType::KEY,
                                tool_key_for_touch_count(active_count),
                                1,
                            ));
                        }

                        // Pre-compute transform values to avoid borrow conflicts
                        let tx = self.transform_x(event.x);
                        let ty = self.transform_y(event.y);
                        let tp = self.transform_pressure(event.pressure);
                        let major: i32;
                        let minor: i32;
                        let orientation = if event.height >= event.width {
                            major = self.transform_touch_size(event.height);
                            minor = self.transform_touch_size(event.width);
                            0
                        } else {
                            major = self.transform_touch_size(event.width);
                            minor = self.transform_touch_size(event.height);
                            1
                        };

                        events.extend_from_slice(&[
                            ev(EventType::ABSOLUTE, EC_ABS_MT_PRESSURE, tp),
                            ev(EventType::ABSOLUTE, EC_ABS_MT_TOUCH_MAJOR, major),
                            ev(EventType::ABSOLUTE, EC_ABS_MT_TOUCH_MINOR, minor),
                            ev(EventType::ABSOLUTE, EC_ABS_MT_ORIENTATION, orientation),
                            ev(EventType::ABSOLUTE, EC_ABS_MT_POSITION_X, tx),
                            ev(EventType::ABSOLUTE, EC_ABS_MT_POSITION_Y, ty),
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_X, tx),
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_Y, ty),
                        ]);

                        emit_events(&mut self.touch, &events);
                    }
                    PointerEventType::CANCEL
                    | PointerEventType::UP
                    | PointerEventType::LEAVE
                    | PointerEventType::OUT => {
                        if let Some(slot) = self.find_slot(event.pointer_id) {
                            // Same live-count derivation as touch-down:
                            // clear the tool key for the current count, and
                            // if touches remain, re-assert the tool key for
                            // the count after this one lifts.
                            let count_before =
                                self.touches.iter().filter(|t| t.is_some()).count();
                            let mut events = vec![
                                ev(EventType::ABSOLUTE, EC_ABS_MT_SLOT, slot as i32),
                                ev(EventType::ABSOLUTE, EC_ABS_MT_TRACKING_ID, -1),
                                ev(EventType::KEY, EC_KEY_TOUCH, 0),
                                ev(
                                    EventType::KEY,
                                    tool_key_for_touch_count(count_before),
                                    0,
                                ),
                            ];
                            let count_after = count_before - 1;
                            if count_after >= 1 {
                                events.push(ev(
                                    EventType::KEY,
                                    tool_key_for_touch_count(count_after),
                                    1,
                                ));
                            }
                            emit_events(&mut self.touch, &events);
                            self.touches[slot] = None;
                        }
                    }
                };
            }
            PointerType::Pen => {
                self.last_pen_event = Instant::now();
                #[cfg(feature = "x11")]
                if self.num_stylus_mapping_tries < MAX_SCREEN_MAPPING_TRIES {
                    if let Some(x11ctx) = &mut self.x11ctx {
                        if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
                            if session_type != "wayland" {
                                x11ctx.map_input_device_to_entire_screen(
                                    &self.name_stylus_device,
                                    true,
                                );
                            }
                        } else {
                            x11ctx
                                .map_input_device_to_entire_screen(&self.name_stylus_device, true);
                        }
                    }
                    self.num_stylus_mapping_tries += 1;
                }
                match event.event_type {
                    PointerEventType::DOWN
                    | PointerEventType::MOVE
                    | PointerEventType::OVER
                    | PointerEventType::ENTER => {
                        let mut events = Vec::new();
                        if let PointerEventType::DOWN = event.event_type {
                            self.pen_touching = true;
                            events.push(ev(EventType::KEY, EC_KEY_TOUCH, 1));
                        }
                        if !self.tool_pen_active && !event.buttons.contains(Button::ERASER) {
                            events.push(ev(EventType::KEY, EC_KEY_TOOL_PEN, 1));
                            events.push(ev(EventType::KEY, EC_KEY_TOOL_RUBBER, 0));
                            self.tool_pen_active = true;
                        }
                        if let Button::ERASER = event.button {
                            events.push(ev(EventType::KEY, EC_KEY_TOOL_PEN, 0));
                            events.push(ev(EventType::KEY, EC_KEY_TOOL_RUBBER, 1));
                            self.tool_pen_active = false;
                        }

                        // Pre-compute transform values
                        let tx = self.transform_x(event.x);
                        let ty = self.transform_y(event.y);
                        let pressure = if self.pen_touching {
                            self.transform_pressure(event.pressure)
                        } else {
                            0
                        };

                        events.extend_from_slice(&[
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_X, tx),
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_Y, ty),
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_PRESSURE, pressure),
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_TILT_X, event.tilt_x),
                            ev(EventType::ABSOLUTE, EC_ABSOLUTE_TILT_Y, event.tilt_y),
                            ev(
                                EventType::ABSOLUTE,
                                EC_ABSOLUTE_DISTANCE,
                                compute_hover_distance(event.pressure as f32),
                            ),
                        ]);

                        emit_events(&mut self.stylus, &events);
                    }
                    PointerEventType::UP
                    | PointerEventType::CANCEL
                    | PointerEventType::LEAVE
                    | PointerEventType::OUT => {
                        emit_events(
                            &mut self.stylus,
                            &[
                                ev(EventType::KEY, EC_KEY_TOUCH, 0),
                                ev(EventType::KEY, EC_KEY_TOOL_PEN, 0),
                                ev(EventType::KEY, EC_KEY_TOOL_RUBBER, 0),
                                ev(EventType::ABSOLUTE, EC_ABSOLUTE_PRESSURE, 0),
                                ev(EventType::ABSOLUTE, EC_ABSOLUTE_DISTANCE, ABS_MAXVAL),
                            ],
                        );
                        self.tool_pen_active = false;
                        self.pen_touching = false;
                    }
                }
            }
            PointerType::Mouse | PointerType::Unknown => {
                #[cfg(feature = "x11")]
                if self.num_mouse_mapping_tries < MAX_SCREEN_MAPPING_TRIES {
                    if let Some(x11ctx) = &mut self.x11ctx {
                        if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
                            if session_type != "wayland" {
                                x11ctx.map_input_device_to_entire_screen(
                                    &self.name_mouse_device,
                                    false,
                                );
                            }
                        } else {
                            x11ctx
                                .map_input_device_to_entire_screen(&self.name_mouse_device, false);
                        }
                    }
                    self.num_mouse_mapping_tries += 1;
                }
                match event.event_type {
                    PointerEventType::DOWN
                    | PointerEventType::MOVE
                    | PointerEventType::OVER
                    | PointerEventType::ENTER => {
                        let mut events = Vec::new();
                        if let PointerEventType::DOWN = event.event_type {
                            match event.button {
                                Button::PRIMARY => {
                                    events.push(ev(EventType::KEY, EC_KEY_MOUSE_LEFT, 1))
                                }
                                Button::SECONDARY => {
                                    events.push(ev(EventType::KEY, EC_KEY_MOUSE_RIGHT, 1))
                                }
                                Button::AUXILARY => {
                                    events.push(ev(EventType::KEY, EC_KEY_MOUSE_MIDDLE, 1))
                                }
                                _ => (),
                            }
                        }

                        // Pre-compute transform values
                        let tx = self.transform_x(event.x);
                        let ty = self.transform_y(event.y);

                        events.push(ev(EventType::ABSOLUTE, EC_ABSOLUTE_X, tx));
                        events.push(ev(EventType::ABSOLUTE, EC_ABSOLUTE_Y, ty));

                        emit_events(&mut self.mouse, &events);
                    }
                    PointerEventType::UP
                    | PointerEventType::CANCEL
                    | PointerEventType::LEAVE
                    | PointerEventType::OUT => {
                        let events: Vec<InputEvent> = match event.button {
                            Button::PRIMARY => {
                                vec![ev(EventType::KEY, EC_KEY_MOUSE_LEFT, 0)]
                            }
                            Button::SECONDARY => {
                                vec![ev(EventType::KEY, EC_KEY_MOUSE_RIGHT, 0)]
                            }
                            Button::AUXILARY => {
                                vec![ev(EventType::KEY, EC_KEY_MOUSE_MIDDLE, 0)]
                            }
                            _ => vec![],
                        };
                        if !events.is_empty() {
                            emit_events(&mut self.mouse, &events);
                        }
                    }
                }
            }
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        use crate::uinput_keys::*;
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        fn map_key(code: &str, location: &KeyboardLocation) -> c_int {
            match (code, location) {
                ("Escape", _) => KEY_ESC,
                ("Digit0", KeyboardLocation::NUMPAD) => KEY_KP0,
                ("Digit1", KeyboardLocation::NUMPAD) => KEY_KP1,
                ("Digit2", KeyboardLocation::NUMPAD) => KEY_KP2,
                ("Digit3", KeyboardLocation::NUMPAD) => KEY_KP3,
                ("Digit4", KeyboardLocation::NUMPAD) => KEY_KP4,
                ("Digit5", KeyboardLocation::NUMPAD) => KEY_KP5,
                ("Digit6", KeyboardLocation::NUMPAD) => KEY_KP6,
                ("Digit7", KeyboardLocation::NUMPAD) => KEY_KP7,
                ("Digit8", KeyboardLocation::NUMPAD) => KEY_KP8,
                ("Digit9", KeyboardLocation::NUMPAD) => KEY_KP9,
                ("Minus", KeyboardLocation::NUMPAD) => KEY_KPMINUS,
                ("Equal", KeyboardLocation::NUMPAD) => KEY_KPEQUAL,
                ("Enter", KeyboardLocation::NUMPAD) => KEY_KPENTER,
                ("Digit0", _) => KEY_0,
                ("Digit1", _) => KEY_1,
                ("Digit2", _) => KEY_2,
                ("Digit3", _) => KEY_3,
                ("Digit4", _) => KEY_4,
                ("Digit5", _) => KEY_5,
                ("Digit6", _) => KEY_6,
                ("Digit7", _) => KEY_7,
                ("Digit8", _) => KEY_8,
                ("Digit9", _) => KEY_9,
                ("Minus", _) => KEY_MINUS,
                ("Equal", _) => KEY_EQUAL,
                ("Enter", _) => KEY_ENTER,
                ("Backspace", _) => KEY_BACKSPACE,
                ("Tab", _) => KEY_TAB,
                ("KeyA", _) => KEY_A,
                ("KeyB", _) => KEY_B,
                ("KeyC", _) => KEY_C,
                ("KeyD", _) => KEY_D,
                ("KeyE", _) => KEY_E,
                ("KeyF", _) => KEY_F,
                ("KeyG", _) => KEY_G,
                ("KeyH", _) => KEY_H,
                ("KeyI", _) => KEY_I,
                ("KeyJ", _) => KEY_J,
                ("KeyK", _) => KEY_K,
                ("KeyL", _) => KEY_L,
                ("KeyM", _) => KEY_M,
                ("KeyN", _) => KEY_N,
                ("KeyO", _) => KEY_O,
                ("KeyP", _) => KEY_P,
                ("KeyQ", _) => KEY_Q,
                ("KeyR", _) => KEY_R,
                ("KeyS", _) => KEY_S,
                ("KeyT", _) => KEY_T,
                ("KeyU", _) => KEY_U,
                ("KeyV", _) => KEY_V,
                ("KeyW", _) => KEY_W,
                ("KeyX", _) => KEY_X,
                ("KeyY", _) => KEY_Y,
                ("KeyZ", _) => KEY_Z,
                ("BracketLeft", _) => KEY_LEFTBRACE,
                ("BracketRight", _) => KEY_RIGHTBRACE,
                ("Semicolon", _) => KEY_SEMICOLON,
                ("Quote", _) => KEY_APOSTROPHE,
                ("Backquote", _) => KEY_GRAVE,
                ("Backslash", _) => KEY_BACKSLASH,
                ("Comma", _) => KEY_COMMA,
                ("Period", _) => KEY_DOT,
                ("Slash", _) => KEY_SLASH,
                ("Space", _) => KEY_SPACE,
                ("CapsLock", _) => KEY_CAPSLOCK,
                ("NumpadMultiply", _) => KEY_KPASTERISK,
                ("F1", _) => KEY_F1,
                ("F2", _) => KEY_F2,
                ("F3", _) => KEY_F3,
                ("F4", _) => KEY_F4,
                ("F5", _) => KEY_F5,
                ("F6", _) => KEY_F6,
                ("F7", _) => KEY_F7,
                ("F8", _) => KEY_F8,
                ("F9", _) => KEY_F9,
                ("F10", _) => KEY_F10,
                ("F11", _) => KEY_F11,
                ("F12", _) => KEY_F12,
                ("F13", _) => KEY_F13,
                ("F14", _) => KEY_F14,
                ("F15", _) => KEY_F15,
                ("F16", _) => KEY_F16,
                ("F17", _) => KEY_F17,
                ("F18", _) => KEY_F18,
                ("F19", _) => KEY_F19,
                ("F20", _) => KEY_F20,
                ("F21", _) => KEY_F21,
                ("F22", _) => KEY_F22,
                ("F23", _) => KEY_F23,
                ("F24", _) => KEY_F24,
                ("NumLock", _) => KEY_NUMLOCK,
                ("ScrollLock", _) => KEY_SCROLLLOCK,
                ("Numpad0", _) => KEY_KP0,
                ("Numpad1", _) => KEY_KP1,
                ("Numpad2", _) => KEY_KP2,
                ("Numpad3", _) => KEY_KP3,
                ("Numpad4", _) => KEY_KP4,
                ("Numpad5", _) => KEY_KP5,
                ("Numpad6", _) => KEY_KP6,
                ("Numpad7", _) => KEY_KP7,
                ("Numpad8", _) => KEY_KP8,
                ("Numpad9", _) => KEY_KP9,
                ("NumpadSubtract", _) => KEY_KPMINUS,
                ("NumpadAdd", _) => KEY_KPPLUS,
                ("IntlBackslash", _) => KEY_102ND,
                ("IntlRo", _) => KEY_RO,
                ("NumpadEnter", _) => KEY_KPENTER,
                ("NumpadDivide", _) => KEY_KPSLASH,
                ("NumpadEqual", _) => KEY_KPEQUAL,
                ("NumpadComma", _) => KEY_KPCOMMA,
                ("NumpadParenLeft", _) => KEY_KPLEFTPAREN,
                ("NumpadParenRight", _) => KEY_KPRIGHTPAREN,
                ("KanaMode", _) => KEY_KATAKANA,
                ("PrintScreen", _) => KEY_SYSRQ,
                ("Home", _) => KEY_HOME,
                ("ArrowUp", _) => KEY_UP,
                ("PageUp", _) => KEY_PAGEUP,
                ("ArrowLeft", _) => KEY_LEFT,
                ("ArrowRight", _) => KEY_RIGHT,
                ("End", _) => KEY_END,
                ("ArrowDown", _) => KEY_DOWN,
                ("PageDown", _) => KEY_PAGEDOWN,
                ("Insert", _) => KEY_INSERT,
                ("Delete", _) => KEY_DELETE,
                ("VolumeMute", _) | ("AudioVolumeMute", _) => KEY_MUTE,
                ("VolumeDown", _) | ("AudioVolumeDown", _) => KEY_VOLUMEDOWN,
                ("VolumeUp", _) | ("AudioVolumeUp", _) => KEY_VOLUMEUP,
                ("Pause", _) => KEY_PAUSE,
                ("Lang1", _) => KEY_HANGUEL,
                ("Lang2", _) => KEY_HANJA,
                ("IntlYen", _) => KEY_YEN,
                ("OSLeft", _) => KEY_LEFTMETA,
                ("OSRight", _) => KEY_RIGHTMETA,
                ("ContextMenu", _) => KEY_MENU,
                ("Cancel", _) => KEY_CANCEL,
                ("Again", _) => KEY_AGAIN,
                ("Props", _) => KEY_PROPS,
                ("Undo", _) => KEY_UNDO,
                ("Copy", _) => KEY_COPY,
                ("Open", _) => KEY_OPEN,
                ("Paste", _) => KEY_PASTE,
                ("Find", _) => KEY_FIND,
                ("Cut", _) => KEY_CUT,
                ("Help", _) => KEY_HELP,
                ("LaunchMail", _) => KEY_MAIL,
                ("Eject", _) => KEY_EJECTCD,
                ("MediaTrackNext", _) => KEY_NEXTSONG,
                ("MediaPlayPause", _) => KEY_PLAYPAUSE,
                ("MediaTrackPrevious", _) => KEY_PREVIOUSSONG,
                ("MediaStop", _) => KEY_STOPCD,
                ("MediaSelect", _) | ("LaunchMediaPlayer", _) => KEY_MEDIA,
                ("Power", _) => KEY_POWER,
                ("Sleep", _) => KEY_SLEEP,
                ("WakeUp", _) => KEY_WAKEUP,
                ("ControlLeft", _) => KEY_LEFTCTRL,
                ("ControlRight", _) => KEY_RIGHTCTRL,
                ("AltLeft", _) => KEY_LEFTALT,
                ("AltRight", _) => KEY_RIGHTALT,
                ("MetaLeft", _) => KEY_LEFTMETA,
                ("MetaRight", _) => KEY_RIGHTMETA,
                ("ShiftLeft", _) => KEY_LEFTSHIFT,
                ("ShiftRight", _) => KEY_RIGHTSHIFT,
                _ => KEY_UNKNOWN,
            }
        }

        let key_code: c_int = map_key(&event.code, &event.location);
        let state: c_int = match event.event_type {
            KeyboardEventType::UP => 0,
            KeyboardEventType::DOWN => 1,
            KeyboardEventType::REPEAT => 2,
        };

        if key_code == KEY_UNKNOWN {
            if let KeyboardEventType::DOWN = event.event_type {
                if !event.key.is_empty() {
                    let unicode_keys = event
                        .key
                        .encode_utf16()
                        .map(|b| format!("{:X}", b))
                        .collect::<Vec<String>>()
                        .concat();

                    debug!(
                        "Got unknown key: {} code: {}, trying to insert unicode using ctrl + \
                        shift + u + {}!",
                        event.code, event.key, unicode_keys
                    );

                    // Press Ctrl+Shift+U to enter unicode input mode
                    emit_events(
                        &mut self.keyboard,
                        &[
                            ev(EventType::KEY, KEY_LEFTCTRL, 1),
                            ev(EventType::KEY, KEY_LEFTSHIFT, 1),
                            ev(EventType::KEY, KEY_U, 1),
                        ],
                    );
                    for c in unicode_keys.chars() {
                        let kc = if c.is_alphabetic() {
                            map_key(&format!("Key{}", c), &KeyboardLocation::STANDARD)
                        } else {
                            map_key(&format!("Digit{}", c), &KeyboardLocation::STANDARD)
                        };

                        emit_events(&mut self.keyboard, &[ev(EventType::KEY, kc, 1)]);
                        emit_events(&mut self.keyboard, &[ev(EventType::KEY, kc, 0)]);
                    }
                    emit_events(
                        &mut self.keyboard,
                        &[
                            ev(EventType::KEY, KEY_LEFTCTRL, 0),
                            ev(EventType::KEY, KEY_LEFTSHIFT, 0),
                            ev(EventType::KEY, KEY_U, 0),
                        ],
                    );
                }
            } else {
                debug!(
                    "Got unknown key: code: {} key: {}, ignoring event.",
                    event.code, event.key
                );
            }
            return;
        }

        if event.ctrl {
            emit_events(
                &mut self.keyboard,
                &[ev(EventType::KEY, KEY_LEFTCTRL, state)],
            );
        }
        if event.alt {
            emit_events(
                &mut self.keyboard,
                &[ev(EventType::KEY, KEY_LEFTALT, state)],
            );
        }
        if event.meta {
            emit_events(
                &mut self.keyboard,
                &[ev(EventType::KEY, KEY_LEFTMETA, state)],
            );
        }
        if event.shift {
            emit_events(
                &mut self.keyboard,
                &[ev(EventType::KEY, KEY_LEFTSHIFT, state)],
            );
        }

        emit_events(&mut self.keyboard, &[ev(EventType::KEY, key_code, state)]);
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable;
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::UInputDevice
    }
}

// Pure math helpers extracted for testability.

fn compute_transform_x(x: f64, geom_x: f64, geom_w: f64) -> i32 {
    // Clamp defensively: the protocol contract says x is 0.0..=1.0, but a
    // client can historically send out-of-range values (e.g. a stale custom
    // input area implementation scaling past the edge), and an unclamped
    // result here would write an out-of-declared-range ABS_X value to uinput.
    ((x * geom_w + geom_x).clamp(0.0, 1.0) * ABS_MAX) as i32
}

fn compute_transform_y(y: f64, geom_y: f64, geom_h: f64) -> i32 {
    ((y * geom_h + geom_y).clamp(0.0, 1.0) * ABS_MAX) as i32
}

fn compute_transform_pressure(p: f64) -> i32 {
    (p * ABS_MAX) as i32
}

fn compute_transform_touch_size(s: f64) -> i32 {
    (s * ABS_MAX) as i32
}

pub fn compute_hover_distance(pressure: f32) -> i32 {
    if pressure > 0.0 {
        0
    } else {
        ABS_MAXVAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error as IoError, ErrorKind};

    // ── Coordinate transforms ───────────────────────────────────────

    #[test]
    fn transform_x_mid_default_geometry() {
        // Default geometry: x=0.0, w=1.0
        // 0.5 * 1.0 + 0.0 = 0.5  =>  0.5 * 65535.0 = 32767.5 => 32767
        assert_eq!(compute_transform_x(0.5, 0.0, 1.0), 32767);
    }

    #[test]
    fn transform_x_boundaries() {
        assert_eq!(compute_transform_x(0.0, 0.0, 1.0), 0);
        assert_eq!(compute_transform_x(1.0, 0.0, 1.0), 65535);
    }

    #[test]
    fn transform_y_mid_default_geometry() {
        assert_eq!(compute_transform_y(0.5, 0.0, 1.0), 32767);
    }

    #[test]
    fn transform_y_boundaries() {
        assert_eq!(compute_transform_y(0.0, 0.0, 1.0), 0);
        assert_eq!(compute_transform_y(1.0, 0.0, 1.0), 65535);
    }

    #[test]
    fn transform_x_with_geometry_offset() {
        // geometry: x=0.25, w=0.5  (captures right-center quarter of screen)
        // input 0.0 => (0.0 * 0.5 + 0.25) * 65535 = 16383.75 => 16383
        assert_eq!(compute_transform_x(0.0, 0.25, 0.5), 16383);
        // input 1.0 => (1.0 * 0.5 + 0.25) * 65535 = 49151.25 => 49151
        assert_eq!(compute_transform_x(1.0, 0.25, 0.5), 49151);
        // input 0.5 => (0.5 * 0.5 + 0.25) * 65535 = 32767.5 => 32767
        assert_eq!(compute_transform_x(0.5, 0.25, 0.5), 32767);
    }

    #[test]
    fn transform_y_with_geometry_offset() {
        // geometry: y=0.1, h=0.8
        // input 0.0 => (0.0 * 0.8 + 0.1) * 65535 = 6553.5 => 6553
        assert_eq!(compute_transform_y(0.0, 0.1, 0.8), 6553);
        // input 1.0 => (1.0 * 0.8 + 0.1) * 65535 = 58981.5 => 58981
        assert_eq!(compute_transform_y(1.0, 0.1, 0.8), 58981);
    }

    #[test]
    fn transform_x_clamps_above_one() {
        // A stale/out-of-range client value must never exceed ABS_MAX.
        assert_eq!(compute_transform_x(1.5, 0.0, 1.0), 65535);
    }

    #[test]
    fn transform_x_clamps_below_zero() {
        assert_eq!(compute_transform_x(-0.5, 0.0, 1.0), 0);
    }

    #[test]
    fn transform_y_clamps_above_one() {
        assert_eq!(compute_transform_y(1.5, 0.0, 1.0), 65535);
    }

    #[test]
    fn transform_y_clamps_below_zero() {
        assert_eq!(compute_transform_y(-0.5, 0.0, 1.0), 0);
    }

    #[test]
    fn transform_x_clamps_when_geometry_pushes_past_range() {
        // In-range x (0.9) combined with an offset geometry can still push
        // the normalized result past 1.0 before ABS scaling.
        assert_eq!(compute_transform_x(0.9, 0.5, 1.0), 65535);
    }

    // ── Pressure transform ──────────────────────────────────────────

    #[test]
    fn transform_pressure_zero() {
        assert_eq!(compute_transform_pressure(0.0), 0);
    }

    #[test]
    fn transform_pressure_max() {
        assert_eq!(compute_transform_pressure(1.0), 65535);
    }

    #[test]
    fn transform_pressure_half() {
        // 0.5 * 65535.0 = 32767.5 => 32767
        assert_eq!(compute_transform_pressure(0.5), 32767);
    }

    // ── Touch size transform ────────────────────────────────────────

    #[test]
    fn transform_touch_size_boundaries() {
        assert_eq!(compute_transform_touch_size(0.0), 0);
        assert_eq!(compute_transform_touch_size(1.0), 65535);
        assert_eq!(compute_transform_touch_size(0.5), 32767);
    }

    // ── Multi-touch slot management ─────────────────────────────────

    /// Helper: create a bare touches array for slot tests. Sized to 5 to
    /// match the real `UInputDevice.touches` field and the ABS_MT_SLOT
    /// range (0..=4) declared in `create_touch()`.
    fn empty_touches() -> [Option<MultiTouch>; 5] {
        Default::default()
    }

    fn find_slot_in(touches: &[Option<MultiTouch>; 5], id: i64) -> Option<usize> {
        touches.iter().enumerate().find_map(|(slot, mt)| match mt {
            Some(mt) if mt.id == id => Some(slot),
            _ => None,
        })
    }

    fn allocate_slot(touches: &mut [Option<MultiTouch>; 5], id: i64) -> Option<usize> {
        if let Some(existing) = find_slot_in(touches, id) {
            return Some(existing);
        }
        let free =
            touches
                .iter()
                .enumerate()
                .find_map(|(slot, mt)| if mt.is_none() { Some(slot) } else { None });
        if let Some(slot) = free {
            touches[slot] = Some(MultiTouch { id });
        }
        free
    }

    #[test]
    fn slot_allocation_sequential() {
        let mut touches = empty_touches();
        assert_eq!(allocate_slot(&mut touches, 100), Some(0));
        assert_eq!(allocate_slot(&mut touches, 200), Some(1));
        assert_eq!(allocate_slot(&mut touches, 300), Some(2));
    }

    #[test]
    fn slot_find_existing() {
        let mut touches = empty_touches();
        allocate_slot(&mut touches, 42);
        allocate_slot(&mut touches, 99);
        // Re-requesting an existing id returns same slot
        assert_eq!(allocate_slot(&mut touches, 42), Some(0));
        assert_eq!(allocate_slot(&mut touches, 99), Some(1));
    }

    #[test]
    fn slot_dealloc_and_reuse() {
        let mut touches = empty_touches();
        allocate_slot(&mut touches, 1);
        allocate_slot(&mut touches, 2);
        allocate_slot(&mut touches, 3);

        // Free slot 1 (index 1)
        touches[1] = None;
        // Next allocation should reuse slot 1
        assert_eq!(allocate_slot(&mut touches, 99), Some(1));
    }

    #[test]
    fn slot_limit_exhaustion() {
        let mut touches = empty_touches();
        for i in 0..5 {
            assert_eq!(allocate_slot(&mut touches, i), Some(i as usize));
        }
        // All 5 slots occupied (matching the declared ABS_MT_SLOT max of 4);
        // a 6th touch is gracefully ignored rather than allocated out of range.
        assert_eq!(allocate_slot(&mut touches, 999), None);
    }

    // ── BTN_TOOL_* live-count derivation ──────────────────────────────

    #[test]
    fn tool_key_for_touch_count_maps_one_through_five() {
        assert_eq!(tool_key_for_touch_count(1), EC_KEY_TOOL_FINGER);
        assert_eq!(tool_key_for_touch_count(2), EC_KEY_TOOL_DOUBLETAP);
        assert_eq!(tool_key_for_touch_count(3), EC_KEY_TOOL_TRIPLETAP);
        assert_eq!(tool_key_for_touch_count(4), EC_KEY_TOOL_QUADTAP);
        assert_eq!(tool_key_for_touch_count(5), EC_KEY_TOOL_QUINTTAP);
    }

    #[test]
    fn tool_key_for_touch_count_saturates_at_quinttap() {
        // No evdev tool code exists past 5 fingers; anything higher (should
        // be unreachable given the 5-slot ceiling) saturates rather than
        // panicking or wrapping.
        assert_eq!(tool_key_for_touch_count(6), EC_KEY_TOOL_QUINTTAP);
    }

    #[test]
    fn tool_transition_survives_out_of_fifo_release() {
        // Reproduces the scenario from the audit: touch A down (slot 0),
        // touch B down (slot 1) -> 2-finger gesture. A lifts (slot 0
        // freed). Touch C lands and is allocated slot 0 (first free slot).
        // The live touch count — not the slot index C landed in — must
        // still report 2 concurrent fingers.
        let mut touches = empty_touches();
        allocate_slot(&mut touches, 1); // A -> slot 0
        allocate_slot(&mut touches, 2); // B -> slot 1
        touches[0] = None; // A lifts, slot 0 freed
        allocate_slot(&mut touches, 3); // C -> reuses slot 0

        let active_count = touches.iter().filter(|t| t.is_some()).count();
        assert_eq!(active_count, 2);
        assert_eq!(tool_key_for_touch_count(active_count), EC_KEY_TOOL_DOUBLETAP);
    }

    // ── Error mapping ───────────────────────────────────────────────

    #[test]
    fn map_io_error_permission_denied() {
        let err = IoError::new(ErrorKind::PermissionDenied, "test");
        let cerr = map_io_error(err, "ctx");
        assert_eq!(cerr.code(), 101);
    }

    #[test]
    fn map_io_error_not_found() {
        let err = IoError::new(ErrorKind::NotFound, "test");
        let cerr = map_io_error(err, "ctx");
        assert_eq!(cerr.code(), 101);
    }

    #[test]
    fn map_io_error_other() {
        let err = IoError::new(ErrorKind::BrokenPipe, "test");
        let cerr = map_io_error(err, "ctx");
        assert_eq!(cerr.code(), 1);
    }

    #[test]
    fn hover_distance_with_pressure_is_zero() {
        assert_eq!(compute_hover_distance(0.5), 0);
        assert_eq!(compute_hover_distance(1.0), 0);
    }

    #[test]
    fn hover_distance_max_pressure_is_zero() {
        assert_eq!(compute_hover_distance(1.0), 0);
    }

    #[test]
    fn hover_distance_no_pressure_is_max() {
        assert_eq!(compute_hover_distance(0.0), ABS_MAXVAL);
    }

    #[test]
    fn hover_distance_clamps_negative_pressure() {
        assert_eq!(compute_hover_distance(-0.5), ABS_MAXVAL);
    }
}
