//! The audio thread allocates and frees nothing (docs/stage-11.md §3.1): a
//! counting allocator around renders in which voices start, loop, change,
//! pause, follow the game's pause, stop, end, are stolen and resampled,
//! 3D voices move around a listener that turns, streamed clips play, and
//! voices and groups fade.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use fynkel_audio::{Audio, Clip, Listener, MAX_VOICES, Play, QUIET_RATE, Rolloff, Settings, Spatial, Streaming, wav};

struct Counting;

// Counted on the measuring thread only: the tests run side by side, and
// one's setup isn't the other's allocation.
thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    if ARMED.with(Cell::get) {
        COUNT.with(|c| c.set(c.get() + 1));
    }
}

// SAFETY: passes every call to the system's allocator, counting.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as the caller promised.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        // SAFETY: as the caller promised.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn counted(work: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    work();
    ARMED.with(|a| a.set(false));
    COUNT.with(Cell::get)
}

#[test]
fn rendering_allocates_nothing_whatever_the_voices_do() {
    let mut audio = Audio::new();
    let short = Arc::new(Clip::new(22_050, 1, vec![0.25; 300]).unwrap());
    let long = Arc::new(Clip::new(QUIET_RATE, 2, vec![0.1; 96_000]).unwrap());
    let mut out = vec![0.0; 512 * 2];
    // The game's side keeps its clips, so their last drops aren't counted;
    // what's measured is the mixer and the game's side between frames.
    let mut handles = Vec::with_capacity(64);
    audio.update(0.0);

    let allocations = counted(|| {
        for round in 0..20 {
            for i in 0..MAX_VOICES + 4 {
                let clip = if i % 2 == 0 { &short } else { &long };
                let play = Play {
                    settings: Settings {
                        volume: 0.5,
                        pitch: 0.5 + (i % 5) as f32 * 0.3,
                        looping: i % 3 == 0,
                        group: (i % 3) as u8,
                    },
                    priority: (i * 7 % 257) as u16,
                    pausable: i % 4 != 0,
                    owner: (i % 3) as u64,
                    from: 0.0,
                    spatial: Spatial { blend: (i % 3) as f32 * 0.5, pan: -0.5, ..Spatial::default() },
                    position: [i as f32, 0.0, -2.0],
                    fade_in: if i % 5 == 0 { 0.1 } else { 0.0 },
                };
                if let Ok(Some(handle)) = audio.play(clip, play)
                    && handles.len() < handles.capacity()
                {
                    handles.push(handle);
                }
            }
            audio.render(&mut out).unwrap();
            for (i, &handle) in handles.iter().enumerate() {
                match i % 4 {
                    0 => audio.pause(handle, round % 2 == 0).unwrap(),
                    1 => audio.set(handle, Settings { volume: 0.2, pitch: 1.5, looping: false, group: 0 }).unwrap(),
                    2 => audio.stop(handle).unwrap(),
                    _ => {
                        let rolloff = if round % 2 == 0 { Rolloff::Linear } else { Rolloff::Logarithmic };
                        audio.set_spatial(handle, Spatial { blend: 1.0, rolloff, ..Spatial::default() }).unwrap();
                    }
                }
                audio.place(handle, [round as f32, 1.0, i as f32 - 10.0]).unwrap();
            }
            let turn = round as f32 * 0.3;
            audio
                .set_listener(Listener {
                    position: [0.0, 1.0, round as f32],
                    rotation: [0.0, turn.sin(), 0.0, turn.cos()],
                })
                .unwrap();
            audio.update(0.0);
            audio.set_game_paused(round % 3 == 0).unwrap();
            audio.render(&mut out).unwrap();
            audio.stop_owner((round % 3) as u64).unwrap();
            handles.clear();
            audio.update(0.02);
        }
    });
    assert_eq!(allocations, 0);
    assert!(audio.stats().stolen > 0);
}

#[test]
fn mixing_streams_and_fades_allocates_nothing() {
    let mut audio = Audio::new();
    let samples: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    let file = wav(QUIET_RATE, 2, &samples);
    let streamed = Arc::new(Clip::load(&file, "music.wav", Streaming::Always).unwrap());
    let looping = Play { settings: Settings { looping: true, group: 1, ..Settings::default() }, ..Play::default() };
    let handles: Vec<_> = (0..4)
        .map(|i| audio.play(&streamed, Play { from: f64::from(i) * 0.1, fade_in: 0.05, ..looping }).unwrap().unwrap())
        .collect();
    let mut out = vec![0.0; 256 * 2];
    // Decoded a second ahead; mixed for less than that, as the audio
    // thread would, while the game fades the voices and their group.
    audio.decode_streams();
    let allocations = counted(|| {
        for round in 0..40 {
            if round % 10 == 0 {
                audio.set_group(1, 0.2 + round as f32 / 100.0, 0.02).unwrap();
                audio.fade(handles[round / 10], 0.5, 0.01, false).unwrap();
            }
            if round == 30 {
                audio.set(handles[1], Settings { looping: false, group: 2, ..Settings::default() }).unwrap();
                audio.fade(handles[0], 0.0, 0.0, true).unwrap();
            }
            audio.render_mixer(&mut out).unwrap();
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(audio.stats().starved, 0);
}
