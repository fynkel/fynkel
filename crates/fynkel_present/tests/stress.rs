//! The ring under the three threads that use it for real: the render
//! worker reserves, the GPU's completion callback completes, and the
//! presenter acquires (and now and then resets, as a resize would).
//!
//! Besides the ring's own invariant checks (on after every operation in
//! debug builds), this test keeps its own record of which slots are being
//! written, and checks from the outside that no slot ever has two writers,
//! no shown slot is being written, frames are shown in order, and at the
//! end every frame is accounted for and every slot still cycles.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Instant;

use fynkel_present::{MAX_IN_FLIGHT, Ring, SLOTS, SlotState, Ticket};

const FRAMES: u64 = 1_000_000;

/// xorshift64*.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }
}

/// (generation, slot) → frames writing into it.
type Writers = Mutex<HashMap<(u64, usize), u32>>;

#[test]
fn three_threads_and_a_million_frames() {
    let ring = Ring::new();
    let writers = Writers::default();
    let gpu_done = AtomicBool::new(false);
    let (to_gpu, gpu) = mpsc::channel::<Ticket>();
    let start = Instant::now();

    let shown = thread::scope(|s| {
        // The render worker.
        s.spawn(|| {
            let mut rng = Rng(1);
            let mut reserved = 0;
            while reserved < FRAMES {
                let Some(ticket) = ring.reserve() else {
                    thread::yield_now();
                    continue;
                };
                reserved += 1;
                let key = (ticket.generation(), ticket.slot());
                let mut w = writers.lock().unwrap();
                let n = w.entry(key).or_default();
                assert_eq!(*n, 0, "two frames writing into slot {key:?}");
                if rng.below(1000) == 0 {
                    drop(w);
                    let _ = ring.cancel(ticket); // stale after a reset is fine
                } else {
                    *n = 1;
                    drop(w);
                    to_gpu.send(ticket).unwrap();
                }
            }
            drop(to_gpu);
        });

        // The GPU's completion callback: sometimes out of order.
        s.spawn(|| {
            let mut rng = Rng(2);
            let mut held: Option<Ticket> = None;
            let finish = |ticket: Ticket| {
                *writers.lock().unwrap().get_mut(&(ticket.generation(), ticket.slot())).unwrap() -= 1;
                let _ = ring.complete(ticket);
            };
            for ticket in gpu {
                if held.is_none() && rng.below(8) == 0 {
                    held = Some(ticket);
                    continue;
                }
                finish(ticket);
                if let Some(older) = held.take() {
                    finish(older);
                }
            }
            if let Some(older) = held.take() {
                finish(older);
            }
            gpu_done.store(true, Ordering::Release);
        });

        // The presenter.
        s.spawn(|| {
            let (mut last_seq, mut shown) = (0, 0u64);
            loop {
                let done = gpu_done.load(Ordering::Acquire);
                match ring.acquire() {
                    Some(frame) if frame.is_new => {
                        assert!(frame.seq > last_seq, "frame {} shown after {last_seq}", frame.seq);
                        last_seq = frame.seq;
                        shown += 1;
                        let w = writers.lock().unwrap();
                        assert_eq!(w.get(&(frame.generation, frame.slot)).copied().unwrap_or(0), 0);
                        drop(w);
                        if shown % 50_000 == 0 {
                            ring.reset();
                        }
                    }
                    // Everything is finished and nothing new was waiting.
                    _ if done => break shown,
                    _ => thread::yield_now(),
                }
            }
        })
        .join()
        .unwrap()
    });
    let elapsed = start.elapsed();

    let stats = ring.stats();
    println!("{stats:?}, {shown} shown, in {elapsed:.2?}");
    assert_eq!(stats.reserved, FRAMES);
    assert_eq!(ring.in_flight(), 0);
    assert_eq!(stats.reserved, stats.completed + stats.cancelled + stats.stale, "a frame was lost");
    let waiting = ring.states().iter().filter(|&&s| s == SlotState::Ready).count() as u64;
    assert_eq!(stats.completed, stats.presented + stats.dropped + stats.discarded + waiting, "a frame was lost");
    assert_eq!(stats.presented, shown);
    assert!(stats.cancelled > 0 && stats.stale > 0 && stats.dropped > 0, "some paths weren't exercised: {stats:?}");
    assert!(writers.lock().unwrap().values().all(|&n| n == 0));

    // No slot was lost: every one still goes round. Two at a time in flight,
    // so the ring has to use all of them to keep the shown and previous slot
    // out of the way.
    let mut used = [false; SLOTS];
    for _ in 0..4 * SLOTS {
        let tickets: Vec<Ticket> = (0..MAX_IN_FLIGHT).map(|_| ring.reserve().unwrap()).collect();
        for ticket in tickets {
            used[ticket.slot()] = true;
            ring.complete(ticket).unwrap();
        }
        ring.acquire().unwrap();
        ring.acquire().unwrap();
    }
    assert_eq!(used, [true; SLOTS], "a slot never came back");
}
