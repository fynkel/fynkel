//! The input contract (blueprint §5.1, §9 "Input", §19.2d item 7; C2, D3).
//!
//! Games read **named actions**, never keys: the project file's
//! `inputActions` declare each action's kind (`button`, `axis1d`,
//! `axis2d`) and the bindings that produce it (`<device>:<control>`), and
//! [`Input::bind`] compiles them into the world's binding table. Hosts
//! feed **raw controls** — one [`InputEvent`] batch per host frame through
//! [`Input::capture`] — and the world resolves them:
//!
//! * **held** state (a button down, an axis value) is what every fixed
//!   tick sees;
//! * **edges** (`pressed`/`released`) captured since the last tick are
//!   pending until a tick runs; [`Input::end_tick`] (called by
//!   `World::step`) consumes them, so exactly one tick sees each edge — the
//!   first tick of a frame, never its catch-up ticks — and a frame that
//!   runs no tick carries them to the next;
//! * `update`/`lateUpdate` see the **frame's** raw edges
//!   (`frame_pressed`/`frame_released`) for presentation-only reactions.
//!
//! The grammar ([`parse_binding`]) is the one place that knows which
//! controls exist; `fynkel validate` checks bindings with it.

use std::collections::HashMap;

/// What an action produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ActionKind {
    /// `pressed` / `held` / `released`.
    Button = 0,
    /// A value in `-1..=1`.
    Axis1d = 1,
    /// A vector in the unit square (mouse deltas are unclamped pixels).
    Axis2d = 2,
}

impl ActionKind {
    pub fn parse(s: &str) -> Option<ActionKind> {
        match s {
            "button" => Some(ActionKind::Button),
            "axis1d" => Some(ActionKind::Axis1d),
            "axis2d" => Some(ActionKind::Axis2d),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ActionKind::Button => "button",
            ActionKind::Axis1d => "axis1d",
            ActionKind::Axis2d => "axis2d",
        }
    }

    pub fn from_u32(v: u32) -> Option<ActionKind> {
        match v {
            0 => Some(ActionKind::Button),
            1 => Some(ActionKind::Axis1d),
            2 => Some(ActionKind::Axis2d),
            _ => None,
        }
    }
}

/// The device a control belongs to (the wire's `device` field).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Device {
    Keyboard = 0,
    Mouse = 1,
    Gamepad = 2,
    Touch = 3,
}

impl Device {
    pub fn parse(s: &str) -> Option<Device> {
        match s {
            "keyboard" => Some(Device::Keyboard),
            "mouse" => Some(Device::Mouse),
            "gamepad" => Some(Device::Gamepad),
            "touch" => Some(Device::Touch),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Device::Keyboard => "keyboard",
            Device::Mouse => "mouse",
            Device::Gamepad => "gamepad",
            Device::Touch => "touch",
        }
    }

    pub fn from_u32(v: u32) -> Option<Device> {
        match v {
            0 => Some(Device::Keyboard),
            1 => Some(Device::Mouse),
            2 => Some(Device::Gamepad),
            3 => Some(Device::Touch),
            _ => None,
        }
    }
}

/// One raw control: a key, a mouse button or axis, a gamepad button or
/// axis, an overlay control — named as the binding grammar names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Control {
    pub device: Device,
    pub name: String,
}

impl Control {
    pub fn new(device: Device, name: impl Into<String>) -> Control {
        Control {
            device,
            name: name.into(),
        }
    }
}

impl std::fmt::Display for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.device.name(), self.name)
    }
}

/// What a raw event says about its control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Phase {
    /// A button went down (a repeat while down is ignored).
    Down = 0,
    /// A button went up.
    Up = 1,
    /// An absolute axis value (`x`, and `y` for 2D controls).
    Axis = 2,
    /// A relative movement, summed over the frame (mouse delta, wheel);
    /// relative controls read zero in a frame with no movement.
    Move = 3,
    /// Focus lost, app paused: every button up (with released edges),
    /// every axis zero. The control is ignored.
    Reset = 4,
}

impl Phase {
    pub fn from_u32(v: u32) -> Option<Phase> {
        match v {
            0 => Some(Phase::Down),
            1 => Some(Phase::Up),
            2 => Some(Phase::Axis),
            3 => Some(Phase::Move),
            4 => Some(Phase::Reset),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::Down => "down",
            Phase::Up => "up",
            Phase::Axis => "axis",
            Phase::Move => "move",
            Phase::Reset => "reset",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        match s {
            "down" => Some(Phase::Down),
            "up" => Some(Phase::Up),
            "axis" => Some(Phase::Axis),
            "move" => Some(Phase::Move),
            "reset" => Some(Phase::Reset),
            _ => None,
        }
    }
}

/// One raw control change, as a host captures it.
#[derive(Clone, Debug, PartialEq)]
pub struct InputEvent {
    pub control: Control,
    pub phase: Phase,
    pub x: f32,
    pub y: f32,
}

impl InputEvent {
    pub fn down(device: Device, name: &str) -> InputEvent {
        InputEvent {
            control: Control::new(device, name),
            phase: Phase::Down,
            x: 1.0,
            y: 0.0,
        }
    }
    pub fn up(device: Device, name: &str) -> InputEvent {
        InputEvent {
            control: Control::new(device, name),
            phase: Phase::Up,
            x: 0.0,
            y: 0.0,
        }
    }
    pub fn axis(device: Device, name: &str, x: f32, y: f32) -> InputEvent {
        InputEvent {
            control: Control::new(device, name),
            phase: Phase::Axis,
            x,
            y,
        }
    }
    pub fn moved(device: Device, name: &str, x: f32, y: f32) -> InputEvent {
        InputEvent {
            control: Control::new(device, name),
            phase: Phase::Move,
            x,
            y,
        }
    }
    pub fn reset() -> InputEvent {
        InputEvent {
            control: Control::new(Device::Keyboard, ""),
            phase: Phase::Reset,
            x: 0.0,
            y: 0.0,
        }
    }
}

/// An action as the project declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub name: String,
    pub kind: ActionKind,
    pub bindings: Vec<String>,
}

// ------------------------------------------------------------------ grammar

/// Key names (`keyboard:<Key>`): letters, digits, and the named keys a
/// game binds. Hosts map their key codes to these names; layouts are the
/// host's business (Flutter reports physical keys).
pub const KEYS: &[&str] = &[
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "Space",
    "Enter",
    "Escape",
    "Tab",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "CapsLock",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
    "MetaLeft",
    "MetaRight",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Backquote",
    "Comma",
    "Period",
    "Slash",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
];

pub const MOUSE_BUTTONS: &[&str] = &["left", "right", "middle", "back", "forward"];
/// Relative 1D mouse control (scroll steps summed over the frame).
pub const MOUSE_AXES_1D: &[&str] = &["wheel"];
/// Relative 2D mouse control (pixels of movement summed over the frame;
/// unclamped).
pub const MOUSE_AXES_2D: &[&str] = &["delta"];

/// Gamepad buttons in the GameController framework's terms (`south` is
/// A/cross, `east` B/circle, `west` X/square, `north` Y/triangle).
pub const GAMEPAD_BUTTONS: &[&str] = &[
    "south",
    "east",
    "west",
    "north",
    "leftShoulder",
    "rightShoulder",
    "leftTrigger",
    "rightTrigger",
    "leftStickButton",
    "rightStickButton",
    "dpadUp",
    "dpadDown",
    "dpadLeft",
    "dpadRight",
    "menu",
    "options",
    "home",
];
/// Spellings accepted for a gamepad button and the name they mean
/// (`start` is the menu button, `select`/`back` the options button).
pub const GAMEPAD_BUTTON_ALIASES: &[(&str, &str)] = &[
    ("start", "menu"),
    ("select", "options"),
    ("back", "options"),
];
pub const GAMEPAD_AXES_1D: &[&str] = &[
    "leftTrigger",
    "rightTrigger",
    "leftStickX",
    "leftStickY",
    "rightStickX",
    "rightStickY",
];
pub const GAMEPAD_AXES_2D: &[&str] = &["leftStick", "rightStick", "dpad"];

/// Named key layouts for `axis2d` (`keyboard:WASD`): left, right, down, up.
pub const KEY_LAYOUTS_2D: &[(&str, [&str; 4])] = &[
    ("WASD", ["A", "D", "S", "W"]),
    (
        "Arrows",
        ["ArrowLeft", "ArrowRight", "ArrowDown", "ArrowUp"],
    ),
    ("IJKL", ["J", "L", "K", "I"]),
];

/// A compiled binding.
#[derive(Clone, Debug, PartialEq)]
pub enum Binding {
    /// A button action from one button control.
    Button(Control),
    /// An `axis1d` from a negative/positive key pair.
    KeyPair { neg: Control, pos: Control },
    /// An `axis2d` from four keys: left, right, down, up.
    KeyQuad([Control; 4]),
    /// An `axis1d`/`axis2d` from a native axis control (stick, trigger,
    /// mouse delta/wheel, overlay stick), multiplied by `scale` (the
    /// grammar's `*<factor>` suffix, 1 without one — how a mouse's pixels
    /// become the units a stick reads in, M3-14).
    Axis { control: Control, scale: f32 },
}

impl Binding {
    /// Every control the binding reads.
    pub fn controls(&self) -> Vec<&Control> {
        match self {
            Binding::Button(c) | Binding::Axis { control: c, .. } => vec![c],
            Binding::KeyPair { neg, pos } => vec![neg, pos],
            Binding::KeyQuad(k) => k.iter().collect(),
        }
    }
}

/// Why a binding does not parse; `Display` is the validation message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// Not `<device>:<control>` with a known device.
    Malformed(String),
    /// A `*<factor>` suffix that is not a positive number, or one on a
    /// binding that is not an axis control.
    BadScale(String),
    UnknownDevice(String),
    /// The device has no such control for this action kind; the message
    /// lists what it has.
    UnknownControl {
        device: Device,
        control: String,
        kind: ActionKind,
        expected: String,
    },
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindingError::Malformed(b) => write!(f, "binding {b:?} is not `<device>:<control>`"),
            BindingError::BadScale(b) => write!(
                f,
                "binding {b:?}: `*<factor>` takes a positive number and scales an axis control (a stick, trigger, mouse delta/wheel or overlay stick)"
            ),
            BindingError::UnknownDevice(d) => write!(
                f,
                "unknown device {d:?} (keyboard, mouse, gamepad or touch)"
            ),
            BindingError::UnknownControl {
                device,
                control,
                kind,
                expected,
            } => write!(
                f,
                "{}:{control} is not {} {} control; {} takes {expected}",
                device.name(),
                if matches!(kind, ActionKind::Axis1d | ActionKind::Axis2d) {
                    "an"
                } else {
                    "a"
                },
                kind.name(),
                device.name()
            ),
        }
    }
}

impl std::error::Error for BindingError {}

fn key(name: &str) -> Option<Control> {
    KEYS.contains(&name)
        .then(|| Control::new(Device::Keyboard, name))
}

/// Parses one `<device>:<control>` binding for an action of `kind`.
pub fn parse_binding(kind: ActionKind, text: &str) -> Result<Binding, BindingError> {
    // `<device>:<control>*<factor>`: the factor scales an axis control.
    let (text, scale) = match text.rsplit_once('*') {
        Some((head, factor)) => {
            let scale: f32 = factor
                .trim()
                .parse()
                .ok()
                .filter(|s: &f32| s.is_finite() && *s > 0.0)
                .ok_or_else(|| BindingError::BadScale(text.to_string()))?;
            (head, Some(scale))
        }
        None => (text, None),
    };
    let binding = parse_control(kind, text)?;
    match (binding, scale) {
        (b, None) => Ok(b),
        (Binding::Axis { control, .. }, Some(scale)) => Ok(Binding::Axis { control, scale }),
        (_, Some(_)) => Err(BindingError::BadScale(text.to_string())),
    }
}

fn parse_control(kind: ActionKind, text: &str) -> Result<Binding, BindingError> {
    let Some((device, control)) = text.split_once(':') else {
        return Err(BindingError::Malformed(text.to_string()));
    };
    if control.is_empty() {
        return Err(BindingError::Malformed(text.to_string()));
    }
    let Some(device) = Device::parse(device) else {
        return Err(BindingError::UnknownDevice(device.to_string()));
    };
    let unknown = |expected: &str| BindingError::UnknownControl {
        device,
        control: control.to_string(),
        kind,
        expected: expected.to_string(),
    };
    let ctl = |name: &str| Control::new(device, name);
    let axis = |control: Control| Binding::Axis {
        control,
        scale: 1.0,
    };
    let one_of = |list: &[&str]| list.contains(&control).then(|| ctl(control));
    let listed = |list: &[&str]| list.join(", ");
    match (device, kind) {
        (Device::Keyboard, ActionKind::Button) => {
            key(control).map(Binding::Button).ok_or_else(|| {
                unknown("a key name (A–Z, 0–9, Space, Enter, Escape, ArrowUp, ShiftLeft, F1, …)")
            })
        }
        (Device::Keyboard, ActionKind::Axis1d) => {
            let parts: Vec<&str> = control.split('/').collect();
            match parts.as_slice() {
                [n, p] => match (key(n), key(p)) {
                    (Some(neg), Some(pos)) => Ok(Binding::KeyPair { neg, pos }),
                    _ => Err(unknown("`<negative key>/<positive key>`, e.g. A/D")),
                },
                _ => Err(unknown("`<negative key>/<positive key>`, e.g. A/D")),
            }
        }
        (Device::Keyboard, ActionKind::Axis2d) => {
            if let Some((_, keys)) = KEY_LAYOUTS_2D.iter().find(|(name, _)| *name == control) {
                return Ok(Binding::KeyQuad(
                    keys.map(|k| Control::new(Device::Keyboard, k)),
                ));
            }
            let parts: Vec<&str> = control.split('/').collect();
            let expected = "WASD, Arrows, IJKL or `<left>/<right>/<down>/<up>` key names";
            match parts.as_slice() {
                [l, r, d, u] => match (key(l), key(r), key(d), key(u)) {
                    (Some(l), Some(r), Some(d), Some(u)) => Ok(Binding::KeyQuad([l, r, d, u])),
                    _ => Err(unknown(expected)),
                },
                _ => Err(unknown(expected)),
            }
        }
        (Device::Mouse, ActionKind::Button) => one_of(MOUSE_BUTTONS)
            .map(Binding::Button)
            .ok_or_else(|| unknown(&listed(MOUSE_BUTTONS))),
        (Device::Mouse, ActionKind::Axis1d) => one_of(MOUSE_AXES_1D)
            .map(axis)
            .ok_or_else(|| unknown(&listed(MOUSE_AXES_1D))),
        (Device::Mouse, ActionKind::Axis2d) => one_of(MOUSE_AXES_2D)
            .map(axis)
            .ok_or_else(|| unknown(&listed(MOUSE_AXES_2D))),
        (Device::Gamepad, ActionKind::Button) => {
            let name = GAMEPAD_BUTTON_ALIASES
                .iter()
                .find(|(alias, _)| *alias == control)
                .map_or(control, |(_, name)| name);
            if GAMEPAD_BUTTONS.contains(&name) {
                Ok(Binding::Button(Control::new(Device::Gamepad, name)))
            } else {
                Err(unknown(&format!(
                    "{} (start = menu, select/back = options)",
                    listed(GAMEPAD_BUTTONS)
                )))
            }
        }
        (Device::Gamepad, ActionKind::Axis1d) => one_of(GAMEPAD_AXES_1D)
            .map(axis)
            .ok_or_else(|| unknown(&listed(GAMEPAD_AXES_1D))),
        (Device::Gamepad, ActionKind::Axis2d) => one_of(GAMEPAD_AXES_2D)
            .map(axis)
            .ok_or_else(|| unknown(&listed(GAMEPAD_AXES_2D))),
        (Device::Touch, kind) => {
            let Some(name) = control.strip_prefix("overlay/") else {
                return Err(unknown("`overlay/<control>`"));
            };
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(unknown(
                    "`overlay/<control>` with an alphanumeric control name",
                ));
            }
            match kind {
                ActionKind::Button => Ok(Binding::Button(ctl(control))),
                ActionKind::Axis2d => Ok(axis(ctl(control))),
                ActionKind::Axis1d => Err(unknown(
                    "`overlay/<control>` for a button (a button) or an axis2d (a stick) only",
                )),
            }
        }
    }
}

// -------------------------------------------------------------------- state

#[derive(Clone, Debug, Default, PartialEq)]
struct Raw {
    down: bool,
    /// Edges since the last tick consumed them.
    pressed: u32,
    released: u32,
    /// Edges since the last capture (the frame's).
    frame_pressed: u32,
    frame_released: u32,
    /// The value (an axis), 1/0 (a button), or — for a relative control —
    /// the motion pending for the next tick (zeroed when a tick consumes it).
    x: f32,
    y: f32,
    /// A relative control's motion summed over the frame (`update`'s view):
    /// zeroed by the next capture.
    frame_x: f32,
    frame_y: f32,
    /// Mouse delta/wheel: motion, not a value (M3-14: consumed by one tick).
    relative: bool,
}

/// One action's state as a tick (or a frame) sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionState {
    pub name: String,
    pub kind: ActionKind,
    /// A button down / an axis away from zero.
    pub held: bool,
    /// Went down since the last tick consumed edges (this tick's).
    pub pressed: bool,
    pub released: bool,
    /// Went down during the current frame's capture (`update`'s view).
    pub frame_pressed: bool,
    pub frame_released: bool,
    /// `axis1d` in `x`; `axis2d` in `x`, `y`; a button 1/0 in `x`: a
    /// rate. A relative control (mouse delta, wheel) contributes the
    /// motion this tick consumes as the rate covering it in one tick
    /// (`x · fixed dt` is the motion; a catch-up tick reads zero).
    pub x: f32,
    pub y: f32,
    /// The same as the frame sees it (`update`'s view): a relative
    /// control's motion summed over the current frame, as motion (not a
    /// rate); keys and sticks read the same as `x`, `y`.
    pub frame_x: f32,
    pub frame_y: f32,
}

struct Action {
    name: String,
    kind: ActionKind,
    bindings: Vec<Binding>,
}

/// A compiled replacement action table with no ownership of raw input state.
/// Preparation has no effect until the plan is published into an input source.
pub struct PreparedInputBindings {
    actions: Vec<Action>,
}

impl PreparedInputBindings {
    /// Compiles using the same grammar and report-and-skip policy as `Input::bind`.
    pub fn prepare(specs: &[ActionSpec]) -> (Self, Vec<(String, String, BindingError)>) {
        let mut errors = Vec::new();
        let mut actions = Vec::with_capacity(specs.len());
        for spec in specs {
            let mut bindings = Vec::new();
            for b in &spec.bindings {
                match parse_binding(spec.kind, b) {
                    Ok(binding) => bindings.push(binding),
                    Err(e) => errors.push((spec.name.clone(), b.clone(), e)),
                }
            }
            actions.push(Action {
                name: spec.name.clone(),
                kind: spec.kind,
                bindings,
            });
        }
        (Self { actions }, errors)
    }
}

/// The binding table and raw control state of one world.
pub struct Input {
    actions: Vec<Action>,
    raw: HashMap<Control, Raw>,
    frames: u64,
    events: u64,
    /// Seconds per fixed tick: a relative control's motion, consumed by
    /// one tick, is reported to that tick as the rate that covers it in
    /// `tick_dt` (`x · dt` is the motion), so an axis always reads a rate.
    tick_dt: f32,
}

impl Default for Input {
    fn default() -> Self {
        Input::with_tick_dt(1.0 / 60.0)
    }
}

impl Input {
    /// A table whose relative controls are rated for ticks of `tick_dt`.
    pub fn with_tick_dt(tick_dt: f32) -> Self {
        Input {
            actions: Vec::new(),
            raw: HashMap::new(),
            frames: 0,
            events: 0,
            tick_dt: if tick_dt > 0.0 { tick_dt } else { 1.0 / 60.0 },
        }
    }

    /// Compiles and publishes a replacement action table. Invalid bindings
    /// are reported and skipped; raw controls and pending edges are preserved.
    pub fn bind(&mut self, specs: &[ActionSpec]) -> Vec<(String, String, BindingError)> {
        let (prepared, errors) = PreparedInputBindings::prepare(specs);
        self.publish_bindings(prepared);
        errors
    }

    /// Publishes only compiled bindings, preserving captured controls, edge
    /// state, capture counters, and the fixed-tick duration.
    pub fn publish_bindings(&mut self, prepared: PreparedInputBindings) {
        self.actions = prepared.actions;
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    pub fn is_bound(&self) -> bool {
        !self.actions.is_empty()
    }

    /// The action kinds, by name.
    pub fn actions(&self) -> impl Iterator<Item = (&str, ActionKind)> + '_ {
        self.actions.iter().map(|a| (a.name.as_str(), a.kind))
    }

    /// The overlay controls the bindings name (`touch:overlay/<name>`),
    /// with the kind of action each drives — what a touch overlay draws.
    pub fn overlay_controls(&self) -> Vec<(String, ActionKind)> {
        let mut out: Vec<(String, ActionKind)> = Vec::new();
        for a in &self.actions {
            for b in &a.bindings {
                for c in b.controls() {
                    if c.device == Device::Touch {
                        let name = c.name.trim_start_matches("overlay/").to_string();
                        if !out.iter().any(|(n, _)| *n == name) {
                            out.push((name, a.kind));
                        }
                    }
                }
            }
        }
        out
    }

    /// Host frames captured and raw events applied so far.
    pub fn counters(&self) -> (u64, u64) {
        (self.frames, self.events)
    }

    /// Applies a host frame's raw events. The frame's edge counters and
    /// the relative controls' frame sums restart; the motion pending for
    /// the next tick (like the pending edges) carries over a frame that
    /// ran no tick.
    pub fn capture(&mut self, events: &[InputEvent]) {
        self.frames += 1;
        for r in self.raw.values_mut() {
            r.frame_pressed = 0;
            r.frame_released = 0;
            r.frame_x = 0.0;
            r.frame_y = 0.0;
        }
        for e in events {
            self.events += 1;
            match e.phase {
                Phase::Reset => {
                    for r in self.raw.values_mut() {
                        if r.down {
                            r.down = false;
                            r.released += 1;
                            r.frame_released += 1;
                        }
                        r.x = 0.0;
                        r.y = 0.0;
                        r.frame_x = 0.0;
                        r.frame_y = 0.0;
                    }
                }
                Phase::Down => {
                    let r = self.raw.entry(e.control.clone()).or_default();
                    if !r.down {
                        r.down = true;
                        r.pressed += 1;
                        r.frame_pressed += 1;
                    }
                    r.x = 1.0;
                }
                Phase::Up => {
                    let r = self.raw.entry(e.control.clone()).or_default();
                    if r.down {
                        r.down = false;
                        r.released += 1;
                        r.frame_released += 1;
                    }
                    r.x = 0.0;
                }
                Phase::Axis => {
                    let r = self.raw.entry(e.control.clone()).or_default();
                    r.relative = false;
                    r.x = finite(e.x);
                    r.y = finite(e.y);
                }
                Phase::Move => {
                    let r = self.raw.entry(e.control.clone()).or_default();
                    r.relative = true;
                    r.x += finite(e.x);
                    r.y += finite(e.y);
                    r.frame_x += finite(e.x);
                    r.frame_y += finite(e.y);
                }
            }
        }
    }

    /// A tick ran: its edges and the relative controls' pending motion are
    /// consumed. Held state stays.
    pub fn end_tick(&mut self) {
        for r in self.raw.values_mut() {
            r.pressed = 0;
            r.released = 0;
            if r.relative {
                r.x = 0.0;
                r.y = 0.0;
            }
        }
    }

    /// Whether a raw control is down / its value (for hosts and tests).
    pub fn raw(&self, control: &Control) -> Option<(bool, f32, f32)> {
        self.raw.get(control).map(|r| (r.down, r.x, r.y))
    }

    /// Every action's state as the next tick (and this frame) sees it.
    pub fn view(&self) -> Vec<ActionState> {
        self.actions.iter().map(|a| self.state_of(a)).collect()
    }

    /// [`Input::view`] into `out`, reusing its states and their name
    /// buffers: a warm view allocates nothing (foundation 4.8).
    pub fn view_into(&self, out: &mut Vec<ActionState>) {
        out.truncate(self.actions.len());
        for (i, a) in self.actions.iter().enumerate() {
            match out.get_mut(i) {
                Some(s) => {
                    if s.name != a.name {
                        s.name.clear();
                        s.name.push_str(&a.name);
                    }
                    self.fill_state(a, s);
                }
                None => out.push(self.state_of(a)),
            }
        }
    }

    /// One action by name.
    pub fn action(&self, name: &str) -> Option<ActionState> {
        self.actions
            .iter()
            .find(|a| a.name == name)
            .map(|a| self.state_of(a))
    }

    fn state_of(&self, a: &Action) -> ActionState {
        let mut s = ActionState {
            name: a.name.clone(),
            kind: a.kind,
            held: false,
            pressed: false,
            released: false,
            frame_pressed: false,
            frame_released: false,
            x: 0.0,
            y: 0.0,
            frame_x: 0.0,
            frame_y: 0.0,
        };
        self.fill_state(a, &mut s);
        s
    }

    /// Every field of `s` but its name, from `a`'s bindings.
    fn fill_state(&self, a: &Action, s: &mut ActionState) {
        let get = |c: &Control| self.raw.get(c);
        let down = |c: &Control| get(c).is_some_and(|r| r.down);
        let unit = |c: &Control| if down(c) { 1.0 } else { 0.0 };
        s.kind = a.kind;
        s.held = false;
        s.pressed = false;
        s.released = false;
        s.frame_pressed = false;
        s.frame_released = false;
        s.x = 0.0;
        s.y = 0.0;
        s.frame_x = 0.0;
        s.frame_y = 0.0;
        let mut clamp = true;
        for b in &a.bindings {
            match b {
                Binding::Button(c) => {
                    if let Some(r) = get(c) {
                        s.held |= r.down;
                        s.pressed |= r.pressed > 0;
                        s.released |= r.released > 0;
                        s.frame_pressed |= r.frame_pressed > 0;
                        s.frame_released |= r.frame_released > 0;
                    }
                }
                Binding::KeyPair { neg, pos } => {
                    let v = unit(pos) - unit(neg);
                    s.x += v;
                    s.frame_x += v;
                }
                Binding::KeyQuad([l, r, d, u]) => {
                    let (x, y) = (unit(r) - unit(l), unit(u) - unit(d));
                    s.x += x;
                    s.y += y;
                    s.frame_x += x;
                    s.frame_y += y;
                }
                Binding::Axis { control, scale } => {
                    if let Some(r) = get(control) {
                        if r.relative {
                            // Motion (this tick's, as a rate; the frame's
                            // as it is), never clamped: pixels are not
                            // a stick.
                            clamp = false;
                            s.x += r.x * scale / self.tick_dt;
                            s.y += r.y * scale / self.tick_dt;
                            s.frame_x += r.frame_x * scale;
                            s.frame_y += r.frame_y * scale;
                        } else {
                            s.x += r.x * scale;
                            s.y += r.y * scale;
                            s.frame_x += r.x * scale;
                            s.frame_y += r.y * scale;
                        }
                    }
                }
            }
        }
        // Keys and sticks read the same in both views; only a relative
        // control's contribution differs (added above).
        match a.kind {
            ActionKind::Button => {
                s.x = if s.held { 1.0 } else { 0.0 };
                s.y = 0.0;
                s.frame_x = s.x;
                s.frame_y = 0.0;
            }
            ActionKind::Axis1d => {
                if clamp {
                    s.x = s.x.clamp(-1.0, 1.0);
                    s.frame_x = s.frame_x.clamp(-1.0, 1.0);
                }
                s.y = 0.0;
                s.frame_y = 0.0;
                s.held = s.x != 0.0;
            }
            ActionKind::Axis2d => {
                if clamp {
                    s.x = s.x.clamp(-1.0, 1.0);
                    s.y = s.y.clamp(-1.0, 1.0);
                    s.frame_x = s.frame_x.clamp(-1.0, 1.0);
                    s.frame_y = s.frame_y.clamp(-1.0, 1.0);
                }
                s.held = s.x != 0.0 || s.y != 0.0;
            }
        }
    }
}

fn finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs() -> Vec<ActionSpec> {
        vec![
            ActionSpec {
                name: "move".into(),
                kind: ActionKind::Axis2d,
                bindings: vec![
                    "keyboard:WASD".into(),
                    "gamepad:leftStick".into(),
                    "touch:overlay/stick".into(),
                ],
            },
            ActionSpec {
                name: "jump".into(),
                kind: ActionKind::Button,
                bindings: vec![
                    "keyboard:Space".into(),
                    "gamepad:south".into(),
                    "touch:overlay/jump".into(),
                ],
            },
            ActionSpec {
                name: "look".into(),
                kind: ActionKind::Axis2d,
                bindings: vec!["mouse:delta".into(), "gamepad:rightStick".into()],
            },
            ActionSpec {
                name: "zoom".into(),
                kind: ActionKind::Axis1d,
                bindings: vec!["mouse:wheel".into(), "keyboard:Minus/Equal".into()],
            },
        ]
    }

    #[test]
    fn grammar_accepts_the_documented_controls_and_rejects_the_rest() {
        assert!(parse_binding(ActionKind::Button, "keyboard:Space").is_ok());
        assert!(parse_binding(ActionKind::Axis2d, "keyboard:Arrows").is_ok());
        assert_eq!(
            parse_binding(ActionKind::Axis2d, "keyboard:J/L/K/I"),
            parse_binding(ActionKind::Axis2d, "keyboard:IJKL")
        );
        assert!(parse_binding(ActionKind::Axis1d, "keyboard:A/D").is_ok());
        assert!(parse_binding(ActionKind::Button, "mouse:left").is_ok());
        assert!(parse_binding(ActionKind::Axis2d, "mouse:delta").is_ok());
        assert!(parse_binding(ActionKind::Axis1d, "gamepad:rightTrigger").is_ok());
        assert!(parse_binding(ActionKind::Button, "gamepad:leftTrigger").is_ok());
        assert!(parse_binding(ActionKind::Axis2d, "touch:overlay/stick").is_ok());
        for (kind, text) in [
            (ActionKind::Button, "keyboard:Spacebar"),
            (ActionKind::Button, "keyboard:WASD"),
            (ActionKind::Axis2d, "keyboard:Space"),
            (ActionKind::Axis1d, "keyboard:A"),
            (ActionKind::Button, "mouse:delta"),
            (ActionKind::Axis2d, "gamepad:south"),
            (ActionKind::Axis1d, "touch:overlay/slider"),
            (ActionKind::Button, "touch:jump"),
            (ActionKind::Button, "touch:overlay/"),
            (ActionKind::Button, "joystick:south"),
            (ActionKind::Button, "Space"),
        ] {
            let e = parse_binding(kind, text).unwrap_err();
            assert!(!e.to_string().is_empty(), "{text}");
        }
        let e = parse_binding(ActionKind::Button, "gamepad:cross").unwrap_err();
        assert_eq!(
            e.to_string(),
            "gamepad:cross is not a button control; gamepad takes south, east, west, north, leftShoulder, rightShoulder, leftTrigger, rightTrigger, leftStickButton, rightStickButton, dpadUp, dpadDown, dpadLeft, dpadRight, menu, options, home (start = menu, select/back = options)"
        );
        assert_eq!(
            parse_binding(ActionKind::Button, "gamepad:start").unwrap(),
            Binding::Button(Control::new(Device::Gamepad, "menu"))
        );
    }

    #[test]
    fn view_into_reuses_a_stale_buffer_and_equals_view() {
        let mut input = Input::with_tick_dt(1.0 / 60.0);
        assert!(input.bind(&specs()).is_empty());
        input.capture(&[
            InputEvent::down(Device::Keyboard, "Space"),
            InputEvent::down(Device::Keyboard, "D"),
        ]);
        // Longer than the actions, with other names in it.
        let mut out = input.view();
        out.reverse();
        out.push(out[0].clone());
        input.view_into(&mut out);
        assert_eq!(out, input.view());
    }

    #[test]
    fn edges_are_consumed_by_one_tick_and_held_state_stays() {
        let mut input = Input::default();
        assert!(input.bind(&specs()).is_empty());
        // Frame 1: Space goes down. Three catch-up ticks: one jump.
        input.capture(&[InputEvent::down(Device::Keyboard, "Space")]);
        let s = input.action("jump").unwrap();
        assert!(s.held && s.pressed && !s.released && s.frame_pressed);
        input.end_tick();
        let s = input.action("jump").unwrap();
        assert!(
            s.held && !s.pressed && s.frame_pressed,
            "catch-up tick: held only"
        );
        input.end_tick();
        assert!(!input.action("jump").unwrap().pressed);
        // Frame 2: nothing new; a zero-tick frame then Space up.
        input.capture(&[]);
        let s = input.action("jump").unwrap();
        assert!(s.held && !s.pressed && !s.frame_pressed);
        input.capture(&[InputEvent::up(Device::Keyboard, "Space")]);
        // No tick ran for that frame: the edge waits for the next tick.
        input.capture(&[]);
        let s = input.action("jump").unwrap();
        assert!(!s.held && s.released && !s.frame_released);
        input.end_tick();
        assert!(!input.action("jump").unwrap().released);
        // A tap inside one frame: pressed and released, not held.
        input.capture(&[
            InputEvent::down(Device::Keyboard, "Space"),
            InputEvent::up(Device::Keyboard, "Space"),
        ]);
        let s = input.action("jump").unwrap();
        assert!(!s.held && s.pressed && s.released);
        // A repeat while down is not an edge.
        input.end_tick();
        input.capture(&[
            InputEvent::down(Device::Keyboard, "Space"),
            InputEvent::down(Device::Keyboard, "Space"),
        ]);
        input.end_tick();
        input.capture(&[InputEvent::down(Device::Keyboard, "Space")]);
        assert!(!input.action("jump").unwrap().pressed);
    }

    #[test]
    fn axes_compose_keys_sticks_and_relative_controls() {
        let mut input = Input::default();
        input.bind(&specs());
        input.capture(&[
            InputEvent::down(Device::Keyboard, "W"),
            InputEvent::down(Device::Keyboard, "D"),
        ]);
        let m = input.action("move").unwrap();
        assert_eq!((m.x, m.y), (1.0, 1.0));
        assert!(m.held);
        // Opposite keys cancel; a stick adds and the sum stays in the square.
        input.capture(&[
            InputEvent::down(Device::Keyboard, "A"),
            InputEvent::axis(Device::Gamepad, "leftStick", 0.5, -0.25),
        ]);
        let m = input.action("move").unwrap();
        assert_eq!((m.x, m.y), (0.5, 0.75));
        input.capture(&[InputEvent::axis(Device::Gamepad, "leftStick", 0.9, 0.0)]);
        assert_eq!(input.action("move").unwrap().x, 0.9);
        // Mouse deltas sum within a frame, unclamped; the motion is
        // consumed by exactly one tick, like an edge (M3-14), which reads
        // it as the rate covering it in one tick (`x · dt` = the motion):
        // the frame's second tick reads zero while `update`'s view keeps
        // the frame's motion.
        input.end_tick();
        input.capture(&[
            InputEvent::moved(Device::Mouse, "delta", 3.0, -1.0),
            InputEvent::moved(Device::Mouse, "delta", 4.0, 0.5),
        ]);
        let l = input.action("look").unwrap();
        assert!(
            (l.x - 420.0).abs() < 1e-3 && (l.y + 30.0).abs() < 1e-3,
            "{l:?}"
        );
        assert_eq!((l.frame_x, l.frame_y), (7.0, -0.5));
        assert!(l.held);
        input.end_tick();
        let l = input.action("look").unwrap();
        assert_eq!((l.x, l.y), (0.0, 0.0), "a catch-up tick sees no motion");
        assert_eq!((l.frame_x, l.frame_y), (7.0, -0.5), "update sees the frame");
        assert!(!l.held);
        input.capture(&[]);
        let l = input.action("look").unwrap();
        assert_eq!(
            (l.frame_x, l.frame_y),
            (0.0, 0.0),
            "a new frame starts at zero"
        );
        // A frame that runs no tick carries its motion to the next tick.
        input.capture(&[InputEvent::moved(Device::Mouse, "delta", 1.0, 1.0)]);
        input.capture(&[InputEvent::moved(Device::Mouse, "delta", 1.0, 0.0)]);
        let l = input.action("look").unwrap();
        assert!(
            (l.x - 120.0).abs() < 1e-3 && (l.y - 60.0).abs() < 1e-3,
            "{l:?}"
        );
        assert_eq!((l.frame_x, l.frame_y), (1.0, 0.0));
        input.end_tick();
        // axis1d: wheel steps and the key pair, clamped; keys read the same
        // in both views.
        input.capture(&[
            InputEvent::moved(Device::Mouse, "wheel", 2.0, 0.0),
            InputEvent::down(Device::Keyboard, "Equal"),
        ]);
        let z = input.action("zoom").unwrap();
        assert!(
            (z.x - 121.0).abs() < 1e-3,
            "a relative control is not clamped: {z:?}"
        );
        assert_eq!(z.frame_x, 3.0);
        input.end_tick();
        let z = input.action("zoom").unwrap();
        assert_eq!((z.x, z.frame_x), (1.0, 3.0));
        input.capture(&[]);
        assert_eq!(input.action("zoom").unwrap().x, 1.0);
        let m = input.action("move").unwrap();
        assert_eq!((m.frame_x, m.frame_y), (m.x, m.y));
        // Reset releases everything with edges.
        input.end_tick();
        input.capture(&[InputEvent::reset()]);
        let z = input.action("zoom").unwrap();
        assert_eq!(z.x, 0.0);
        let m = input.action("move").unwrap();
        assert!(!m.held);
        assert_eq!(
            input.raw(&Control::new(Device::Keyboard, "W")),
            Some((false, 0.0, 0.0))
        );
    }

    #[test]
    fn a_binding_scale_turns_pixels_into_units() {
        let mut input = Input::with_tick_dt(0.02);
        let specs = vec![
            ActionSpec {
                name: "look".into(),
                kind: ActionKind::Axis2d,
                bindings: vec!["mouse:delta*0.01".into(), "gamepad:rightStick".into()],
            },
            ActionSpec {
                name: "throttle".into(),
                kind: ActionKind::Axis1d,
                bindings: vec!["gamepad:rightTrigger*0.5".into()],
            },
        ];
        assert!(input.bind(&specs).is_empty());
        input.capture(&[
            InputEvent::moved(Device::Mouse, "delta", 200.0, -100.0),
            InputEvent::axis(Device::Gamepad, "rightStick", 0.5, 0.0),
            InputEvent::axis(Device::Gamepad, "rightTrigger", 1.0, 0.0),
        ]);
        let l = input.action("look").unwrap();
        // 2 units of motion in a 20 ms tick is a rate of 100/s, plus the
        // stick's 0.5; the frame view holds the motion itself.
        assert!(
            (l.x - 100.5).abs() < 1e-3 && (l.y + 50.0).abs() < 1e-3,
            "{l:?}"
        );
        assert!((l.frame_x - 2.5).abs() < 1e-6 && (l.frame_y + 1.0).abs() < 1e-6);
        assert_eq!(input.action("throttle").unwrap().x, 0.5);
        input.end_tick();
        assert_eq!(
            input.action("look").unwrap().x,
            0.5,
            "the stick stays, the motion is spent"
        );
        for bad in [
            "mouse:delta*0",
            "mouse:delta*x",
            "keyboard:WASD*2",
            "mouse:delta*-1",
        ] {
            assert!(
                matches!(
                    parse_binding(ActionKind::Axis2d, bad),
                    Err(BindingError::BadScale(_))
                ),
                "{bad}"
            );
        }
        assert!(matches!(
            parse_binding(ActionKind::Button, "keyboard:Space*2"),
            Err(BindingError::BadScale(_))
        ));
    }

    #[test]
    fn overlay_controls_and_binding_errors_are_reported() {
        let mut input = Input::default();
        let errors = input.bind(&[
            ActionSpec {
                name: "fire".into(),
                kind: ActionKind::Button,
                bindings: vec![
                    "mouse:left".into(),
                    "keyboard:Fire".into(),
                    "touch:overlay/fire".into(),
                ],
            },
            ActionSpec {
                name: "move".into(),
                kind: ActionKind::Axis2d,
                bindings: vec!["touch:overlay/stick".into()],
            },
        ]);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            (errors[0].0.as_str(), errors[0].1.as_str()),
            ("fire", "keyboard:Fire")
        );
        assert_eq!(
            input.overlay_controls(),
            vec![
                ("fire".to_string(), ActionKind::Button),
                ("stick".to_string(), ActionKind::Axis2d)
            ]
        );
        // The good bindings still work.
        input.capture(&[InputEvent::down(Device::Touch, "overlay/fire")]);
        assert!(input.action("fire").unwrap().pressed);
        input.capture(&[InputEvent::axis(Device::Touch, "overlay/stick", -0.3, 0.6)]);
        let m = input.action("move").unwrap();
        assert_eq!((m.x, m.y), (-0.3, 0.6));
        assert_eq!(input.counters(), (2, 2));
    }
}
