//! The engine on the null backend: the voice limit with priorities, bus
//! mixing, the tick clock, pause/resume, the clip registry, the mixer's
//! returns dropped on the world's thread.

use std::sync::Arc;

use fynkel_audio::{
    Audio, AudioBus, AudioClip, PlayError, PlayParams, MAX_VOICES, NULL_CHANNELS, NULL_SAMPLE_RATE,
};
use fynkel_contracts::{AssetId, AssetRef, Entity};

fn r(n: u128) -> AssetRef {
    AssetRef::new(AssetId::from_bytes(n.to_le_bytes()))
}

fn constant(rate: u32, channels: u32, frames: usize, value: f32) -> Arc<AudioClip> {
    AudioClip::new(rate, channels, vec![value; frames * channels as usize]).unwrap()
}

fn rms(out: &[f32]) -> f32 {
    (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt()
}

/// Renders past the 5 ms gain smoothing (40 ms, eight time constants)
/// and returns the last frame's left and right samples.
fn settle(audio: &mut Audio, out: &mut [f32]) -> (f32, f32) {
    for _ in 0..4 {
        audio.render(out);
    }
    (out[out.len() - 2], out[out.len() - 1])
}

fn params(priority: i32) -> PlayParams {
    PlayParams {
        priority,
        ..Default::default()
    }
}

#[test]
fn the_voice_limit_steals_by_priority_and_refuses() {
    let mut audio = Audio::new();
    audio
        .register_clip(r(1), constant(48_000, 1, 48_000, 0.25))
        .unwrap();
    let mut ids = Vec::new();
    for i in 0..MAX_VOICES {
        let p = if i < 4 { 0 } else { 5 };
        ids.push(audio.play(None, &r(1), params(p)).unwrap());
    }
    assert_eq!(audio.voice_count(), MAX_VOICES);
    assert_eq!(audio.stats().started, MAX_VOICES as u64);

    // A priority-3 sound steals the oldest priority-0 voice.
    let stolen = audio.play(None, &r(1), params(3)).unwrap();
    assert_eq!(stolen.slot, ids[0].slot);
    assert_ne!(stolen.generation, ids[0].generation);
    assert!(!audio.is_playing(ids[0]));
    assert!(audio.is_playing(ids[1]));
    assert_eq!(audio.stats().stolen, 1);

    // Then the next-oldest priority-0 ones; equal priority steals too.
    audio.play(None, &r(1), params(0)).unwrap();
    audio.play(None, &r(1), params(0)).unwrap();
    audio.play(None, &r(1), params(0)).unwrap();
    assert!(!audio.is_playing(ids[3]));
    assert!(audio.is_playing(ids[4]));

    // Now every voice is ≥ 3 with priority 0 among the newest; a priority
    // -1 sound is refused.
    assert_eq!(audio.play(None, &r(1), params(-1)), Err(PlayError::Refused));
    assert_eq!(audio.stats().refused, 1);
    assert_eq!(audio.voice_count(), MAX_VOICES);

    // An unknown clip.
    assert_eq!(
        audio.play(None, &r(9), params(9)),
        Err(PlayError::UnknownClip)
    );

    // A stale handle does nothing.
    assert!(!audio.stop(ids[0]));
    assert!(!audio.set_volume(ids[0], 0.5));
    assert!(audio.stop(stolen));
    assert_eq!(audio.voice_count(), MAX_VOICES - 1);
}

#[test]
fn buses_mute_and_listener_scale_the_mix() {
    let mut audio = Audio::new();
    // Half a second of a constant 0.5 (mono) and 0.25 (stereo).
    audio
        .register_clip(r(1), constant(48_000, 1, 24_000, 0.5))
        .unwrap();
    audio
        .register_clip(r(2), constant(48_000, 2, 24_000, 0.25))
        .unwrap();
    let mut out = vec![0.0f32; 480 * NULL_CHANNELS];

    // Silence before anything plays.
    audio.render(&mut out);
    assert_eq!(rms(&out), 0.0);

    let sfx = audio
        .play(
            None,
            &r(1),
            PlayParams {
                bus: AudioBus::Sfx,
                ..Default::default()
            },
        )
        .unwrap();
    // Past the 5 ms fade-in, then measure.
    let (sfx_level, _) = settle(&mut audio, &mut out);
    assert!((sfx_level - 0.5).abs() < 0.01, "sfx alone {sfx_level}");

    let music = audio
        .play(
            None,
            &r(2),
            PlayParams {
                bus: AudioBus::Music,
                ..Default::default()
            },
        )
        .unwrap();
    let (l, r) = settle(&mut audio, &mut out);
    assert!((l - 0.75).abs() < 0.01, "both {l}");
    assert!((r - 0.75).abs() < 0.01);

    // The music bus at half.
    audio.set_bus(AudioBus::Music, 0.5);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.625).abs() < 0.01, "music half {l}");

    // Master scales everything, the listener too.
    audio.set_bus(AudioBus::Master, 0.5);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.3125).abs() < 0.01, "master half {l}");
    audio.set_listener_gain(0.5);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.15625).abs() < 0.01, "listener half {l}");

    // Muted is silence; unmuted comes back.
    audio.set_muted(true);
    let (l, _) = settle(&mut audio, &mut out);
    assert!(l.abs() < 1e-3, "muted {l}");
    audio.set_muted(false);
    audio.set_bus(AudioBus::Master, 1.0);
    audio.set_listener_gain(1.0);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.625).abs() < 0.01, "level {l}");

    // A voice's own volume and a pause.
    audio.set_volume(sfx, 0.5);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.375).abs() < 0.01, "sfx half {l}");
    audio.pause(music);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.25).abs() < 0.01, "music paused {l}");
    audio.resume(music);
    let (l, _) = settle(&mut audio, &mut out);
    assert!((l - 0.375).abs() < 0.01, "music resumed {l}");

    // A stop fades to silence and frees the voice on both sides.
    audio.stop(sfx);
    audio.stop(music);
    let (l, _) = settle(&mut audio, &mut out);
    assert!(l.abs() < 1e-3, "stopped {l}");
    assert_eq!(audio.voice_count(), 0);
    let stats = audio.stats();
    assert!(!stats.device);
    assert_eq!(stats.sample_rate, NULL_SAMPLE_RATE);
    assert_eq!(stats.channels, NULL_CHANNELS as u32);
    assert!(stats.callbacks >= 20);
    assert_eq!(stats.clips, 2);
}

#[test]
fn the_tick_clock_ends_voices_and_wraps_loops() {
    let mut audio = Audio::new();
    // One second at 44.1 kHz.
    audio
        .register_clip(r(1), constant(44_100, 1, 44_100, 0.1))
        .unwrap();
    let e = Entity::from_bits(7);
    let once = audio.play(Some(e), &r(1), PlayParams::default()).unwrap();
    let looped = audio
        .play(
            None,
            &r(1),
            PlayParams {
                looping: true,
                pitch: 2.0,
                ..Default::default()
            },
        )
        .unwrap();
    let paused = audio.play(None, &r(1), PlayParams::default()).unwrap();
    audio.pause(paused);
    assert_eq!(audio.voice_of(e), Some(once));

    let dt = 1.0 / 60.0;
    let mut ended = Vec::new();
    let mut tick = 0;
    for t in 0..120 {
        let e = audio.advance(dt);
        if !e.is_empty() && tick == 0 {
            tick = t + 1;
        }
        ended.extend(e);
    }
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(ended[0].voice, once);
    assert_eq!(ended[0].source, Some(e));
    assert_eq!(ended[0].clip, r(1));
    assert_eq!(tick, 60, "one second is 60 ticks");
    assert!(!audio.is_playing(once));
    assert_eq!(audio.voice_of(e), None);

    let s = audio.state(looped).unwrap();
    assert!(s.looping);
    // Two seconds at pitch 2 is four loops: back near the start.
    assert!(s.time < 0.05, "looped time {}", s.time);
    assert_eq!(s.duration, 1.0);
    let p = audio.state(paused).unwrap();
    assert!(p.paused);
    assert_eq!(p.time, 0.0);
    assert_eq!(audio.stats().ended, 1);
    assert_eq!(audio.voices().len(), 2);
}

#[test]
fn clips_are_registered_replaced_and_released() {
    let mut audio = Audio::new();
    let a = constant(48_000, 1, 480, 0.5);
    let b = constant(48_000, 2, 480, 0.5);
    audio.register_clip(r(1), a.clone()).unwrap();
    assert_eq!(Arc::strong_count(&a), 3, "engine + mixer command");
    let id = audio.play(None, &r(1), PlayParams::default()).unwrap();
    let mut out = vec![0.0f32; 96];
    audio.render(&mut out);
    assert_eq!(Arc::strong_count(&a), 3, "engine + mixer slot");

    // Replacing the clip stops its voice and releases the old reference
    // back through the ring once rendered.
    audio.register_clip(r(1), b.clone()).unwrap();
    assert!(!audio.is_playing(id));
    audio.render(&mut out);
    audio.advance(0.0);
    assert_eq!(Arc::strong_count(&a), 1, "released and dropped here");
    assert!(audio.has_clip(&r(1)));
    assert_eq!(audio.clip(&r(1)).map(|c| c.channels), Some(2));

    assert!(audio.unregister_clip(&r(1)));
    assert!(!audio.unregister_clip(&r(1)));
    audio.render(&mut out);
    audio.advance(0.0);
    assert_eq!(Arc::strong_count(&b), 1);
    assert_eq!(audio.clip_count(), 0);

    // A clip the mixer cannot play.
    assert!(AudioClip::new(48_000, 3, vec![0.0; 3]).is_err());
    assert!(AudioClip::new(0, 1, vec![0.0; 3]).is_err());
    assert!(AudioClip::new(48_000, 2, vec![0.0; 3]).is_err());

    // Without the `device` feature the device cannot open; the null
    // backend stays usable.
    #[cfg(not(feature = "device"))]
    {
        assert!(audio.open_device().is_err());
        assert!(!audio.has_device());
    }
    audio.suspend(true);
    assert!(audio.suspended());
    audio.close_device();
    audio.register_clip(r(2), a.clone()).unwrap();
    assert!(audio.play(None, &r(2), PlayParams::default()).is_ok());
}

/// A PCM16 WAV of `frames` mono/stereo frames whose left sample is the
/// frame index / 32768 (so a read tells where it came from).
fn ramp_wav(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    let data_len = frames * channels as u32 * 2;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    w.extend_from_slice(&(channels * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let s = (i % 32_768) as i16;
        w.extend_from_slice(&s.to_le_bytes());
        for _ in 1..channels {
            w.extend_from_slice(&(-s).to_le_bytes());
        }
    }
    w
}

/// Renders until the stream delivers (its decoder runs on its own
/// thread) or a second passes; the output of the delivering render.
fn render_until_sound(audio: &mut Audio, out: &mut [f32]) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        audio.render(out);
        if out.iter().any(|s| *s != 0.0) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    false
}

#[test]
fn a_streamed_clip_is_decoded_as_it_plays_loops_by_seek_and_ends_by_the_clock() {
    let mut audio = Audio::new();
    // A quarter second at the null rate, stereo, so the mixer reads it
    // one to one and the ramp shows through.
    let frames = NULL_SAMPLE_RATE / 4;
    let bytes = ramp_wav(NULL_SAMPLE_RATE, 2, frames);
    let clip = AudioClip::streamed(NULL_SAMPLE_RATE, 2, frames as u64, "pcm", bytes).unwrap();
    assert!(clip.is_stream());
    assert_eq!(clip.frames(), frames as u64);
    audio.register_clip(r(1), clip).unwrap();
    assert!(audio.is_stream(&r(1)));
    assert_eq!(audio.stats().streams, 1);

    // Not looping: plays through, the clock ends it at a quarter second.
    let e = Entity::from_bits(3);
    let once = audio.play(Some(e), &r(1), PlayParams::default()).unwrap();
    let mut out = vec![0.0f32; 256 * NULL_CHANNELS];
    assert!(render_until_sound(&mut audio, &mut out), "no sound came");
    // The first non-silent render: the ramp from its start, after the
    // 5 ms gain ramp, and the right channel the negative of the left.
    let last = out.len() - 2;
    assert!(out[last] > 0.0, "left {}", out[last]);
    assert!(
        (out[last + 1] + out[last]).abs() < 1e-3,
        "right {}",
        out[last + 1]
    );
    let mut ended = Vec::new();
    for _ in 0..30 {
        ended.extend(audio.advance(1.0 / 60.0));
    }
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].voice, once);
    assert!(!audio.is_playing(once));
    // The mixer side reaches the end as well (its slot freed) and never
    // starved: the decoder is a second ahead.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        audio.render(&mut out);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(audio.stats().starved, 0);

    // Looping: still sounding well past the clip's length, the clock
    // wrapped, and a stop ends the decoder.
    let looped = audio
        .play(
            None,
            &r(1),
            PlayParams {
                looping: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(render_until_sound(&mut audio, &mut out));
    for _ in 0..40 {
        audio.advance(1.0 / 60.0);
    }
    // Drain a second of frames (four clip lengths) and check the last
    // render is still sound.
    for _ in 0..(NULL_SAMPLE_RATE as usize / 256) {
        audio.render(&mut out);
        while out.iter().all(|s| *s == 0.0) {
            std::thread::sleep(std::time::Duration::from_millis(2));
            audio.render(&mut out);
        }
    }
    assert!(out.iter().any(|s| *s != 0.0));
    let s = audio.state(looped).unwrap();
    assert!(s.time < 0.25, "wrapped time {}", s.time);
    assert!(audio.stop(looped));
    assert!(!audio.is_playing(looped));
    for _ in 0..20 {
        audio.render(&mut out); // the fade, then the freed slot
    }
    assert!(out.iter().all(|s| *s == 0.0), "silent after the stop");
    let stats = audio.stats();
    assert_eq!(stats.started, 2);
    assert_eq!(stats.ended, 1);
    assert_eq!(stats.streams, 1);

    // A voice resumed at a position (what a device open does) starts
    // there: the ramp reads the position.
    let late = audio.play(None, &r(1), PlayParams::default()).unwrap();
    assert!(render_until_sound(&mut audio, &mut out));
    for _ in 0..6 {
        audio.advance(1.0 / 60.0); // 0.1 s in = frame 4 800
    }
    audio.close_device(); // a no-op without one: the mixer stays
    assert!((audio.state(late).unwrap().time - 0.1).abs() < 1e-3);
    // Unregistering stops the voice and drops the stream's clip.
    assert!(audio.unregister_clip(&r(1)));
    assert!(!audio.is_playing(late));
    assert_eq!(audio.stats().streams, 0);
}
