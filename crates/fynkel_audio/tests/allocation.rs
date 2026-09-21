//! The gate's "no allocation in the audio callback": a counting global
//! allocator around `Mixer::render` with clips loaded, voices starting,
//! stopping, looping, resampling and being stolen, plus a full ring of
//! commands pumped inside the render.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use fynkel_audio::mixer::{Command, Mixer, MixerStats, Return, Start};
use fynkel_audio::{AudioBus, AudioClip, MAX_VOICES};

struct Counting;

static COUNT: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn tone(rate: u32, channels: u32, seconds: f32, hz: f32) -> Arc<AudioClip> {
    let frames = (rate as f32 * seconds) as usize;
    let mut samples = Vec::with_capacity(frames * channels as usize);
    for i in 0..frames {
        let s = (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.5;
        for _ in 0..channels {
            samples.push(s);
        }
    }
    AudioClip::new(rate, channels, samples).unwrap()
}

#[test]
fn render_never_allocates() {
    let (mut cmd_tx, cmd_rx) = rtrb::RingBuffer::new(256);
    let (ret_tx, mut ret_rx) = rtrb::RingBuffer::new(64);
    let stats = Arc::new(MixerStats::default());
    let mut mixer = Mixer::new(48_000, 2, cmd_rx, ret_tx, stats.clone());
    let mono = tone(44_100, 1, 0.05, 440.0);
    let stereo = tone(48_000, 2, 0.5, 220.0);
    let mut out = vec![0.0f32; 512 * 2];

    // Warm-up with allocation allowed: nothing here should allocate either,
    // but only the armed section is asserted.
    mixer.render(&mut out);

    cmd_tx
        .push(Command::LoadClip {
            slot: 0,
            clip: mono.clone(),
        })
        .unwrap();
    cmd_tx
        .push(Command::LoadClip {
            slot: 1,
            clip: stereo.clone(),
        })
        .unwrap();
    for v in 0..MAX_VOICES as u8 {
        cmd_tx
            .push(Command::Start(Start {
                voice: v,
                generation: 1,
                clip: (v % 2) as u16,
                bus: if v % 3 == 0 {
                    AudioBus::Music
                } else {
                    AudioBus::Sfx
                },
                volume: 0.8,
                pitch: 0.5 + v as f32 * 0.1,
                looping: v % 4 == 0,
                position: 0.0,
            }))
            .unwrap();
    }
    cmd_tx
        .push(Command::SetBus {
            bus: AudioBus::Master,
            gain: 0.9,
        })
        .unwrap();
    cmd_tx.push(Command::SetListener(0.7)).unwrap();
    cmd_tx
        .push(Command::Stop {
            voice: 3,
            generation: 1,
        })
        .unwrap();
    cmd_tx
        .push(Command::Pause {
            voice: 5,
            generation: 1,
        })
        .unwrap();
    cmd_tx
        .push(Command::SetVolume {
            voice: 6,
            generation: 1,
            volume: 0.2,
        })
        .unwrap();
    cmd_tx
        .push(Command::SetPitch {
            voice: 7,
            generation: 1,
            pitch: 2.0,
        })
        .unwrap();
    // Voice 1 restarted (stolen) and clip 0 replaced mid-stream.
    cmd_tx
        .push(Command::Start(Start {
            voice: 1,
            generation: 2,
            clip: 1,
            bus: AudioBus::Sfx,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            position: 100.0,
        }))
        .unwrap();
    cmd_tx
        .push(Command::LoadClip {
            slot: 0,
            clip: mono.clone(),
        })
        .unwrap();
    cmd_tx.push(Command::UnloadClip { slot: 1 }).unwrap();
    cmd_tx
        .push(Command::LoadClip {
            slot: 1,
            clip: stereo.clone(),
        })
        .unwrap();
    cmd_tx.push(Command::SetMuted(false)).unwrap();

    COUNT.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    // Enough renders for every non-looping voice to end (0.5 s at 48 kHz
    // is 24 000 frames; 512 a render).
    for _ in 0..120 {
        mixer.render(&mut out);
    }
    ARMED.store(false, Ordering::Relaxed);
    assert_eq!(COUNT.load(Ordering::Relaxed), 0, "the render allocated");

    // Sanity: the mix was not silent and the returns arrived — the two
    // released clips and the ended voices.
    let mut released = 0;
    let mut ended = 0;
    while let Ok(r) = ret_rx.pop() {
        match r {
            Return::ClipReleased(_) => released += 1,
            Return::VoiceEnded { .. } => ended += 1,
        }
    }
    assert_eq!(released, 2);
    // The unload killed every voice on clip 1 silently (the engine had
    // already forgotten them); clip 0's eight non-looping voices ended.
    assert_eq!(ended, 8);
    assert_eq!(stats.callbacks.load(Ordering::Relaxed), 121);
}
