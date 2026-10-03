//! The stand-in without the `engine` feature (ADR-0029 `modules.strip`):
//! the engine's API over nothing. Clips are only remembered (the loader's
//! registrations still succeed), every `play` is refused, there is no
//! mixer, no decoder and no device — no rtrb, symphonia or cpal. The world
//! never steps it: a runtime built without the engine runs with the audio
//! module off (`Modules::COMPILED`).

use std::collections::HashMap;
use std::sync::Arc;

use fynkel_contracts::{AssetRef, Entity};

use crate::clip::AudioClip;
use crate::components::AudioBus;
use crate::types::{
    AudioStats, DeviceInfo, Ended, PlayError, PlayParams, VoiceId, VoiceState, MAX_CLIPS,
    MAX_VOICES, NULL_CHANNELS, NULL_SAMPLE_RATE,
};

pub struct Audio {
    clips: HashMap<AssetRef, Arc<AudioClip>>,
    bus_gain: [f32; AudioBus::COUNT],
    muted: bool,
    listener: f32,
    suspended: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    pub fn retain_clips(&mut self, mut keep: impl FnMut(&AssetRef) -> bool) {
        self.clips.retain(|r, _| keep(r));
    }

    pub fn new() -> Audio {
        Audio {
            clips: HashMap::new(),
            bus_gain: [1.0; AudioBus::COUNT],
            muted: false,
            listener: 1.0,
            suspended: false,
        }
    }

    pub fn register_clip(&mut self, r: AssetRef, clip: Arc<AudioClip>) -> Result<(), String> {
        if !self.clips.contains_key(&r) && self.clips.len() >= MAX_CLIPS {
            return Err(format!("no clip slot free ({MAX_CLIPS} clips loaded)"));
        }
        self.clips.insert(r, clip);
        Ok(())
    }

    pub fn unregister_clip(&mut self, r: &AssetRef) -> bool {
        self.clips.remove(r).is_some()
    }

    pub fn has_clip(&self, r: &AssetRef) -> bool {
        self.clips.contains_key(r)
    }

    pub fn clip(&self, r: &AssetRef) -> Option<&Arc<AudioClip>> {
        self.clips.get(r)
    }

    pub fn clip_count(&self) -> usize {
        self.clips.len()
    }

    pub fn is_stream(&self, r: &AssetRef) -> bool {
        self.clips.get(r).is_some_and(|c| c.is_stream())
    }

    /// Always refused: there is no voice to give.
    pub fn play_at(
        &mut self,
        source: Option<Entity>,
        clip: &AssetRef,
        params: PlayParams,
        _start: u64,
    ) -> Result<VoiceId, PlayError> {
        self.play(source, clip, params)
    }

    pub fn play(
        &mut self,
        _source: Option<Entity>,
        clip: &AssetRef,
        _params: PlayParams,
    ) -> Result<VoiceId, PlayError> {
        if self.clips.contains_key(clip) {
            Err(PlayError::Refused)
        } else {
            Err(PlayError::UnknownClip)
        }
    }

    pub fn is_playing(&self, _id: VoiceId) -> bool {
        false
    }

    pub fn stop(&mut self, _id: VoiceId) -> bool {
        false
    }

    pub fn pause(&mut self, _id: VoiceId) -> bool {
        false
    }

    pub fn resume(&mut self, _id: VoiceId) -> bool {
        false
    }

    pub fn set_volume(&mut self, _id: VoiceId, _volume: f32) -> bool {
        false
    }

    pub fn set_pitch(&mut self, _id: VoiceId, _pitch: f32) -> bool {
        false
    }

    pub fn set_looping(&mut self, _id: VoiceId, _looping: bool) -> bool {
        false
    }

    pub fn voice_of(&self, _source: Entity) -> Option<VoiceId> {
        None
    }

    pub fn voice_sources(&self) -> impl Iterator<Item = (VoiceId, Entity)> + '_ {
        std::iter::empty()
    }

    pub fn state(&self, _id: VoiceId) -> Option<VoiceState> {
        None
    }

    pub fn voices(&self) -> Vec<VoiceState> {
        Vec::new()
    }

    pub fn active_clips(&self) -> impl Iterator<Item = &AssetRef> {
        std::iter::empty()
    }

    pub fn voice_count(&self) -> usize {
        0
    }

    pub fn set_bus(&mut self, bus: AudioBus, gain: f32) {
        if gain.is_finite() {
            self.bus_gain[bus.index()] = gain.clamp(0.0, 1.0);
        }
    }

    pub fn bus(&self, bus: AudioBus) -> f32 {
        self.bus_gain[bus.index()]
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn set_listener_gain(&mut self, gain: f32) {
        if gain.is_finite() {
            self.listener = gain.clamp(0.0, 1.0);
        }
    }

    pub fn listener_gain(&self) -> f32 {
        self.listener
    }

    pub fn advance(&mut self, _dt: f32) -> Vec<Ended> {
        Vec::new()
    }

    /// Silence.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
    }

    pub fn has_device(&self) -> bool {
        false
    }

    pub fn device_info(&self) -> DeviceInfo {
        DeviceInfo {
            sample_rate: NULL_SAMPLE_RATE,
            channels: NULL_CHANNELS,
            device: false,
        }
    }

    pub fn open_device(&mut self) -> Result<DeviceInfo, String> {
        Err("built without the audio engine (modules.strip with audio off)".to_string())
    }

    pub fn close_device(&mut self) {}

    pub fn suspend(&mut self, suspended: bool) {
        self.suspended = suspended;
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn stats(&self) -> AudioStats {
        AudioStats {
            sample_rate: NULL_SAMPLE_RATE,
            channels: NULL_CHANNELS as u32,
            max_voices: MAX_VOICES as u32,
            clips: self.clips.len() as u32,
            suspended: self.suspended,
            streams: self.clips.values().filter(|c| c.is_stream()).count() as u32,
            ..AudioStats::default()
        }
    }
}
