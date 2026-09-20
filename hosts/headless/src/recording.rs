//! Input recordings (`*.input.json`, blueprint §19.2d item 7): a host
//! frame's `dt` and raw control events, one entry per frame, replayed by
//! `fynkel test --headless --input <file>` (this host) and by
//! `package:fynkel`'s `InputRecording.replay` (a Dart headless game). The
//! two readers accept the same document; `fynkel_flutter` writes it.

use std::path::Path;

use fynkel_core::input::{Control, Device, InputEvent, Phase};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "input-recording";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Recording {
    /// `"input-recording"`.
    pub fynkel: String,
    #[serde(rename = "formatVersion")]
    pub format_version: u32,
    pub frames: Vec<RecordedFrame>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecordedFrame {
    /// Seconds handed to `begin_frame`.
    pub dt: f32,
    #[serde(default)]
    pub events: Vec<RecordedEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecordedEvent {
    /// `keyboard`, `mouse`, `gamepad`, `touch`; ignored by a `reset`.
    #[serde(default)]
    pub device: String,
    /// The control's name in the binding grammar (`W`, `left`, `leftStick`,
    /// `overlay/jump`); empty for a `reset`.
    #[serde(default)]
    pub control: String,
    /// `down`, `up`, `axis`, `move`, `reset`.
    pub phase: String,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

impl Recording {
    pub fn read(path: &Path) -> Result<Recording, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Recording::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Recording, String> {
        let r: Recording = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if r.fynkel != FORMAT {
            return Err(format!("`fynkel` is `{}`, expected `{FORMAT}`", r.fynkel));
        }
        if r.format_version != FORMAT_VERSION {
            return Err(format!(
                "formatVersion {} is not {FORMAT_VERSION}",
                r.format_version
            ));
        }
        for (i, f) in r.frames.iter().enumerate() {
            if !(f.dt.is_finite() && f.dt >= 0.0) {
                return Err(format!(
                    "frames[{i}].dt must be a finite, non-negative number"
                ));
            }
            for (j, e) in f.events.iter().enumerate() {
                e.decode()
                    .map_err(|m| format!("frames[{i}].events[{j}]: {m}"))?;
            }
        }
        Ok(r)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("recording is serialisable")
    }

    pub fn events(&self, frame: usize) -> Vec<InputEvent> {
        self.frames[frame]
            .events
            .iter()
            .filter_map(|e| e.decode().ok())
            .collect()
    }
}

impl RecordedEvent {
    pub fn decode(&self) -> Result<InputEvent, String> {
        let phase = Phase::parse(&self.phase).ok_or_else(|| {
            format!(
                "`{}` is not a phase (down, up, axis, move, reset)",
                self.phase
            )
        })?;
        if phase == Phase::Reset {
            return Ok(InputEvent::reset());
        }
        let device = Device::parse(&self.device).ok_or_else(|| {
            format!(
                "`{}` is not a device (keyboard, mouse, gamepad, touch)",
                self.device
            )
        })?;
        if self.control.is_empty() {
            return Err("control is empty".to_string());
        }
        // A button's value is its state, whatever the file says.
        let (x, y) = match phase {
            Phase::Down => (1.0, 0.0),
            Phase::Up => (0.0, 0.0),
            _ => (self.x, self.y),
        };
        Ok(InputEvent {
            control: Control::new(device, self.control.clone()),
            phase,
            x,
            y,
        })
    }

    pub fn from_event(e: &InputEvent) -> RecordedEvent {
        RecordedEvent {
            device: e.control.device.name().to_string(),
            control: e.control.name.clone(),
            phase: e.phase.name().to_string(),
            x: e.x,
            y: e.y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects() {
        let r = Recording::parse(
            r#"{"fynkel":"input-recording","formatVersion":1,"frames":[
                {"dt":0.016,"events":[{"device":"keyboard","control":"W","phase":"down"}]},
                {"dt":0.016},
                {"dt":0.016,"events":[{"phase":"reset"},{"device":"gamepad","control":"leftStick","phase":"axis","x":0.5,"y":-1}]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(r.frames.len(), 3);
        assert_eq!(r.events(0), vec![InputEvent::down(Device::Keyboard, "W")]);
        assert!(r.events(1).is_empty());
        assert_eq!(r.events(2)[0], InputEvent::reset());
        assert_eq!(r.events(2)[1].x, 0.5);
        assert!(
            Recording::parse(r#"{"fynkel":"x","formatVersion":1,"frames":[]}"#)
                .unwrap_err()
                .contains("expected `input-recording`")
        );
        assert!(Recording::parse(
            r#"{"fynkel":"input-recording","formatVersion":1,"frames":[{"dt":0.1,"events":[{"device":"piano","control":"C","phase":"down"}]}]}"#
        )
        .unwrap_err()
        .contains("frames[0].events[0]"));
        assert!(Recording::parse(
            r#"{"fynkel":"input-recording","formatVersion":1,"frames":[{"dt":-1}]}"#
        )
        .unwrap_err()
        .contains("dt"));
    }
}
