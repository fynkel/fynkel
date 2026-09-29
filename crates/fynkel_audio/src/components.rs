//! The audio components (blueprint §9, §19.2d item 11): pure data,
//! authored in scenes, crossing the bridge with the generated codecs. A
//! `fynkel.AudioSource` is a clip with its playback settings on an entity;
//! a `fynkel.AudioListener` marks the ears. Tier 1 is 2D: nothing is
//! panned or attenuated by distance yet (spatial audio is M5), so the
//! listener carries only a gain over everything the world plays.

use fynkel_contracts::AssetRef;
use fynkel_schema_derive::Schema;

/// The bus a voice is mixed into (`docs/audio.md`): `master` scales the
/// other two, the `fynkel.Settings` volumes are the bus gains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Schema)]
#[schema(name = "fynkel.AudioBus")]
pub enum AudioBus {
    /// Everything, on top of the other buses; a source on `master` is
    /// scaled by the master volume only.
    Master,
    /// Music: streamed and looping clips, usually one at a time.
    Music,
    /// Sound effects: footsteps, pickups, impacts.
    #[default]
    Sfx,
}

impl AudioBus {
    pub const COUNT: usize = 3;

    pub fn index(self) -> usize {
        match self {
            AudioBus::Master => 0,
            AudioBus::Music => 1,
            AudioBus::Sfx => 2,
        }
    }
}

/// Lowest pitch multiplier a source may ask for.
pub const MIN_PITCH: f32 = 0.05;
/// Highest pitch multiplier a source may ask for.
pub const MAX_PITCH: f32 = 8.0;

/// A clip with its playback settings on the entity. `playing: true` in a
/// scene starts the clip when the scene loads (like an `Animator`); the
/// world clears it when a non-looping clip ends (`PLAYBACK_ENDED`). A
/// behaviour drives it with the `Audio` command (`play`, `stop`, `pause`,
/// `resume`, `setVolume`, `setPitch`) or by writing the component: a
/// changed `volume`/`pitch`/`looping` reaches the playing voice at the
/// next tick, a changed `playing` starts or stops it. The clip must be an
/// audio source the world was given (`World::register_sound`; the loaders
/// register every clip a loaded scene's sources name, `SOUND_UNAVAILABLE`
/// once otherwise).
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 23, name = "fynkel.AudioSource")]
pub struct AudioSource {
    /// The audio asset (a `.wav`/`.ogg` source; no sub-asset).
    #[fynkel(kind = "asset(sound)", order = 0)]
    pub clip: AssetRef,
    #[fynkel(
        order = 1,
        tooltip = "The bus this source plays through: master, music or sfx."
    )]
    pub bus: AudioBus,
    /// Linear gain, `0..=1`.
    #[fynkel(kind = "slider", min = 0, max = 1, step = 0.01, order = 2)]
    pub volume: f32,
    /// Playback rate multiplier, `MIN_PITCH..=MAX_PITCH`; `1` is the
    /// clip's own rate.
    #[fynkel(
        min = 0.05,
        max = 8,
        step = 0.01,
        order = 3,
        tooltip = "Playback rate; 1 is the clip's own rate."
    )]
    pub pitch: f32,
    #[fynkel(group = "Playback", order = 4)]
    pub looping: bool,
    /// Whether the clip is playing (authored `true`: from the load).
    #[fynkel(group = "Playback", order = 5)]
    pub playing: bool,
    /// Who keeps a voice when the voice limit is reached: a new sound
    /// steals the voice of the lowest-priority sound at or below its own
    /// priority (the oldest among equals) and is refused when every voice
    /// outranks it.
    #[fynkel(
        group = "Playback",
        order = 6,
        tooltip = "Past the voice limit a new sound steals the lowest-priority voice at or below its own."
    )]
    pub priority: i32,
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            clip: AssetRef::default(),
            bus: AudioBus::Sfx,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            playing: false,
            priority: 0,
        }
    }
}

impl AudioSource {
    /// The value rules (`FYN-H0010` in a document, `INVALID_PAYLOAD` at the
    /// tick): a finite gain in `0..=1`, a pitch in `MIN_PITCH..=MAX_PITCH`.
    pub fn validate(&self) -> Result<(), String> {
        validate_volume(self.volume)?;
        validate_pitch(self.pitch)
    }
}

/// Where the ears are. Tier 1 plays everything as 2D, so the listener is
/// a gain over every voice (`0..=1`; fade the world out on a death) and
/// the place spatial audio (M5) will measure from. One per world: the
/// lowest-numbered entity with one is the listener; without one the world
/// still plays at gain 1.
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 24, name = "fynkel.AudioListener")]
pub struct AudioListener {
    /// Linear gain over everything, `0..=1`.
    #[fynkel(kind = "slider", min = 0, max = 1, step = 0.01)]
    pub volume: f32,
}

impl Default for AudioListener {
    fn default() -> Self {
        Self { volume: 1.0 }
    }
}

impl AudioListener {
    pub fn validate(&self) -> Result<(), String> {
        validate_volume(self.volume)
    }
}

pub fn validate_volume(v: f32) -> Result<(), String> {
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        return Err(format!("volume must be in 0..=1, got {v}"));
    }
    Ok(())
}

pub fn validate_pitch(p: f32) -> Result<(), String> {
    if !p.is_finite() || !(MIN_PITCH..=MAX_PITCH).contains(&p) {
        return Err(format!(
            "pitch must be in {MIN_PITCH}..={MAX_PITCH}, got {p}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_rules() {
        assert!(AudioSource::default().validate().is_ok());
        assert!(AudioSource {
            volume: 1.5,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(AudioSource {
            pitch: 0.0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(AudioSource {
            pitch: f32::NAN,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(AudioListener { volume: -0.1 }.validate().is_err());
        assert!(AudioListener::default().validate().is_ok());
    }
}
