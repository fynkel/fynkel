//! The Inspector's sound preview (ADR-0030 step 6): a stage without a
//! world. It holds the cooked clip and an audio engine of its own that
//! opens the output device on the first play. It answers the waveform
//! as peaks computed here, so no samples cross into the host. The
//! playhead runs on the engine's clock, advanced by the wall-clock time
//! between the host's calls.

use std::sync::Arc;
use std::time::Instant;

use fynkel_audio::{Audio, AudioClip, PlayParams, VoiceId};
use fynkel_contracts::{AssetRef, Diagnostic};
use serde_json::{json, Value};

use super::{f32_arg, invalid, Outcome};

/// The most columns a waveform is asked for.
const MAX_COLUMNS: usize = 8192;

pub(super) struct SoundSlot {
    clip_ref: AssetRef,
    clip: Arc<AudioClip>,
    /// Made on the first play.
    audio: Option<Audio>,
    voice: Option<VoiceId>,
    looping: bool,
    /// When the engine's clock was last advanced.
    last: Instant,
}

impl SoundSlot {
    pub(super) fn new(clip_ref: AssetRef, clip: Arc<AudioClip>) -> SoundSlot {
        SoundSlot {
            clip_ref,
            clip,
            audio: None,
            voice: None,
            looping: false,
            last: Instant::now(),
        }
    }

    /// What the open result and `stats` say about the sound.
    pub(super) fn json(&self) -> Value {
        let c = &self.clip;
        json!({
            "subject": "sound",
            "sound": {
                "sampleRate": c.sample_rate,
                "channels": c.channels,
                "frames": c.frames(),
                "duration": c.duration(),
                "streamed": c.is_stream(),
                "codec": c.stream.as_ref().map(|(s, _)| s.codec.as_str()).unwrap_or("pcm"),
            },
        })
    }

    /// The engine's clock brought to now; the voice forgotten once it
    /// ended.
    fn advance(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        let Some(audio) = &mut self.audio else {
            return;
        };
        audio.advance(dt);
        if let Some(v) = self.voice {
            if !audio.is_playing(v) {
                self.voice = None;
            }
        }
    }

    fn playhead(&mut self) -> Value {
        self.advance();
        let time = match (&self.audio, self.voice) {
            (Some(a), Some(v)) => a.state(v).map(|s| s.time),
            _ => None,
        };
        json!({
            "playing": time.is_some(),
            "time": time.unwrap_or(0.0),
            "duration": self.clip.duration(),
            "loop": self.looping,
            "device": self.audio.as_ref().is_some_and(Audio::has_device),
        })
    }

    pub(super) fn call(&mut self, kind: &str, args: &Value) -> Outcome {
        match kind {
            "peaks" => {
                let columns = args
                    .get("columns")
                    .and_then(Value::as_u64)
                    .filter(|c| (1..=MAX_COLUMNS as u64).contains(c))
                    .ok_or_else(|| {
                        invalid(format!("peaks: `columns` must be 1 to {MAX_COLUMNS}"))
                    })? as usize;
                let peaks = fynkel_audio::peaks(&self.clip, columns)
                    .map_err(|e| invalid(format!("peaks: {e}")))?;
                Ok(json!({ "channels": self.clip.channels, "columns": columns, "peaks": peaks }))
            }
            "play" => {
                self.advance();
                if let Some(l) = args.get("loop").and_then(Value::as_bool) {
                    self.looping = l;
                }
                let from = match args.get("from") {
                    Some(v) if !v.is_null() => f32_arg(args, "from")?.max(0.0),
                    _ => 0.0,
                };
                let device = args.get("device").and_then(Value::as_bool).unwrap_or(true);
                if self.audio.is_none() {
                    let mut audio = Audio::new();
                    audio
                        .register_clip(self.clip_ref.clone(), self.clip.clone())
                        .map_err(|e| invalid(format!("play: {e}")))?;
                    self.audio = Some(audio);
                }
                let audio = self.audio.as_mut().expect("made above");
                // No device (none on this machine, or the host asked for
                // none): the clock still runs, silently.
                if device && !audio.has_device() {
                    let _ = audio.open_device();
                }
                if let Some(v) = self.voice.take() {
                    audio.stop(v);
                }
                let start = (from as f64 * self.clip.sample_rate as f64) as u64;
                let params = PlayParams {
                    looping: self.looping,
                    ..PlayParams::default()
                };
                let voice = audio
                    .play_at(None, &self.clip_ref, params, start)
                    .map_err(|e| invalid(format!("play: {e:?}")))?;
                self.voice = Some(voice);
                Ok(self.playhead())
            }
            "stop" => {
                self.advance();
                if let (Some(a), Some(v)) = (&mut self.audio, self.voice.take()) {
                    a.stop(v);
                }
                Ok(self.playhead())
            }
            "loop" => {
                let looping = args
                    .get("loop")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| invalid("loop: `loop` must be a bool"))?;
                self.looping = looping;
                self.advance();
                if let (Some(a), Some(v)) = (&mut self.audio, self.voice) {
                    a.set_looping(v, looping);
                }
                Ok(self.playhead())
            }
            "playhead" => Ok(self.playhead()),
            "stats" | "reload" => Ok(self.json()),
            other => Err(unknown(other)),
        }
    }
}

fn unknown(kind: &str) -> Vec<Diagnostic> {
    vec![Diagnostic::error(
        fynkel_contracts::Code::SESSION_UNKNOWN_QUERY,
        format!("stage: unknown call `{kind}` for a sound"),
    )
    .with_suggestion("one of: peaks, play, stop, loop, playhead, stats")]
}
