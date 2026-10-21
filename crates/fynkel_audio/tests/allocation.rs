//! The audio thread allocates and frees nothing (docs/stage-11.md §3.1): a
//! counting allocator around renders in which voices start, loop, change,
//! pause, follow the game's pause, stop, end, are stolen and resampled,
//! and 3D voices move around a listener that turns.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use fynkel_audio::{Audio, Clip, Listener, MAX_VOICES, Play, QUIET_RATE, Rolloff, Settings, Spatial};

struct Counting;

static COUNT: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);

// SAFETY: passes every call to the system's allocator, counting.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as the caller promised.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: as the caller promised.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn counted(work: impl FnOnce()) -> usize {
    COUNT.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    work();
    ARMED.store(false, Ordering::Relaxed);
    COUNT.load(Ordering::Relaxed)
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
                    settings: Settings { volume: 0.5, pitch: 0.5 + (i % 5) as f32 * 0.3, looping: i % 3 == 0 },
                    priority: (i * 7 % 257) as u16,
                    pausable: i % 4 != 0,
                    owner: (i % 3) as u64,
                    from: 0.0,
                    spatial: Spatial { blend: (i % 3) as f32 * 0.5, pan: -0.5, ..Spatial::default() },
                    position: [i as f32, 0.0, -2.0],
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
                    1 => audio.set(handle, Settings { volume: 0.2, pitch: 1.5, looping: false }).unwrap(),
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
