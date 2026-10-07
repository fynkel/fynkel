use super::*;
use SlotState::*;

/// The cycle from the plan, plus the two ways a frame leaves it early.
const MOVES: [(SlotState, SlotState); 7] = [
    (Free, Rendering),
    (Rendering, Ready),
    (Rendering, Free),
    (Ready, Presented),
    (Ready, Free),
    (Presented, Retiring),
    (Retiring, Free),
];

#[test]
fn the_move_table_has_exactly_the_planned_moves() {
    for from in SlotState::ALL {
        for to in SlotState::ALL {
            assert_eq!(from.can_become(to), MOVES.contains(&(from, to)), "{from:?} → {to:?}");
        }
    }
}

#[test]
fn every_move_is_checked_and_an_illegal_one_changes_nothing() {
    for from in SlotState::ALL {
        for to in SlotState::ALL {
            let ring = Ring::new();
            let mut slots = ring.lock();
            slots.state[2] = from;
            let before = slots.state;
            let result = slots.advance(2, to);
            if MOVES.contains(&(from, to)) {
                assert_eq!(result, Ok(()));
                let mut after = before;
                after[2] = to;
                assert_eq!(slots.state, after);
            } else {
                assert_eq!(result, Err(RingError::Illegal { slot: 2, from, to }));
                assert_eq!(slots.state, before, "{from:?} → {to:?} changed a slot");
            }
        }
    }
}

/// Reserves a frame and finishes it.
fn finished(ring: &Ring) -> u64 {
    let ticket = ring.reserve().unwrap();
    let seq = ticket.seq();
    ring.complete(ticket).unwrap();
    seq
}

#[test]
fn reserve_moves_free_to_rendering() {
    let ring = Ring::new();
    let ticket = ring.reserve().unwrap();
    assert_eq!((ticket.slot(), ticket.generation(), ticket.seq()), (0, 0, 1));
    assert_eq!(ring.states(), [Rendering, Free, Free, Free, Free]);
    assert_eq!(ring.in_flight(), 1);
    assert_eq!(ring.stats().reserved, 1);
    ring.cancel(ticket).unwrap();
}

#[test]
fn complete_moves_rendering_to_ready() {
    let ring = Ring::new();
    finished(&ring);
    assert_eq!(ring.states(), [Ready, Free, Free, Free, Free]);
    assert_eq!(ring.in_flight(), 0);
    assert_eq!(ring.stats().completed, 1);
}

#[test]
fn cancel_moves_rendering_to_free() {
    let ring = Ring::new();
    let ticket = ring.reserve().unwrap();
    ring.cancel(ticket).unwrap();
    assert_eq!(ring.states(), [Free; SLOTS]);
    assert_eq!(ring.in_flight(), 0);
    assert_eq!(ring.stats().cancelled, 1);
}

#[test]
fn acquire_before_the_first_frame_shows_nothing() {
    assert_eq!(Ring::new().acquire(), None);
}

#[test]
fn acquire_moves_ready_to_presented_then_retiring_then_free() {
    let ring = Ring::new();
    finished(&ring);
    let shown = ring.acquire().unwrap();
    assert_eq!(shown, Shown { slot: 0, generation: 0, seq: 1, is_new: true, more: false });
    assert_eq!(ring.states(), [Presented, Free, Free, Free, Free]);

    finished(&ring);
    assert_eq!(ring.acquire().unwrap().slot, 1);
    assert_eq!(ring.states(), [Retiring, Presented, Free, Free, Free]);

    finished(&ring);
    assert_eq!(ring.acquire().unwrap().slot, 2);
    assert_eq!(ring.states(), [Free, Retiring, Presented, Free, Free]);
    assert_eq!(ring.stats().presented, 3);
}

#[test]
fn acquire_without_a_new_frame_shows_the_same_one_again() {
    let ring = Ring::new();
    finished(&ring);
    finished(&ring);
    ring.acquire();
    ring.acquire();
    let again = ring.acquire().unwrap();
    assert_eq!(again, Shown { slot: 1, generation: 0, seq: 2, is_new: false, more: false });
    // The previous slot stays Retiring: nothing newer replaced the shown one.
    assert_eq!(ring.states(), [Retiring, Presented, Free, Free, Free]);
    assert_eq!(ring.stats().presented, 2);
}

#[test]
fn acquire_shows_the_oldest_finished_frame_and_says_more_are_waiting() {
    let ring = Ring::new();
    let (a, b) = (ring.reserve().unwrap(), ring.reserve().unwrap());
    ring.complete(a).unwrap();
    ring.complete(b).unwrap();
    let first = ring.acquire().unwrap();
    assert_eq!((first.seq, first.more), (1, true));
    let second = ring.acquire().unwrap();
    assert_eq!((second.seq, second.more, second.is_new), (2, false, true));
    assert_eq!(ring.stats().dropped, 0);
}

#[test]
fn acquire_drops_the_frames_beyond_the_queue() {
    let ring = Ring::new();
    for _ in 0..MAX_QUEUED + 1 {
        finished(&ring);
    }
    let shown = ring.acquire().unwrap();
    assert_eq!((shown.seq, shown.more), (2, true), "frame 1 is dropped, frame 2 shown, frame 3 waits");
    assert_eq!(ring.states(), [Free, Presented, Ready, Free, Free]);
    assert_eq!(ring.stats().dropped, 1);
}

#[test]
fn reserve_takes_the_slot_that_has_been_free_longest() {
    let ring = Ring::new();
    finished(&ring);
    ring.acquire();
    finished(&ring);
    ring.acquire();
    finished(&ring);
    ring.acquire(); // slot 0 freed just now; 3 and 4 never used
    assert_eq!(ring.states(), [Free, Retiring, Presented, Free, Free]);
    let ticket = ring.reserve().unwrap();
    assert_eq!(ticket.slot(), 3, "slot 0 was the compositor's until a moment ago");
    ring.cancel(ticket).unwrap();
    // A cancelled slot was free only for a moment: it goes to the back too.
    let ticket = ring.reserve().unwrap();
    assert_eq!(ticket.slot(), 4);
    ring.cancel(ticket).unwrap();
}

#[test]
fn reserve_refuses_more_than_max_in_flight() {
    let ring = Ring::new();
    let tickets: Vec<Ticket> = (0..MAX_IN_FLIGHT).map(|_| ring.reserve().unwrap()).collect();
    assert_eq!(ring.reserve(), None);
    assert_eq!(ring.stats().reserved, MAX_IN_FLIGHT as u64, "a refusal reserves nothing");
    for ticket in tickets {
        ring.complete(ticket).unwrap();
    }
    assert!(ring.reserve().is_some_and(|t| ring.cancel(t).is_ok()));
}

#[test]
fn reserve_reuses_the_oldest_waiting_frame_when_no_slot_is_free() {
    let ring = Ring::new();
    finished(&ring);
    ring.acquire();
    finished(&ring);
    ring.acquire(); // slot 0 Retiring, slot 1 Presented
    for _ in 0..3 {
        finished(&ring); // slots 2, 3, 4 Ready
    }
    assert_eq!(ring.states(), [Retiring, Presented, Ready, Ready, Ready]);

    let ticket = ring.reserve().unwrap();
    assert_eq!(ticket.slot(), 2, "the oldest waiting frame (3) gives up its slot");
    assert_eq!(ring.states(), [Retiring, Presented, Rendering, Ready, Ready]);
    assert_eq!(ring.stats().dropped, 1);
    ring.complete(ticket).unwrap();
}

#[test]
fn a_frame_finishing_after_a_newer_one_was_shown_is_dropped() {
    let ring = Ring::new();
    let (old, new) = (ring.reserve().unwrap(), ring.reserve().unwrap());
    ring.complete(new).unwrap();
    assert_eq!(ring.acquire().unwrap().seq, 2);
    ring.complete(old).unwrap();
    assert_eq!(ring.states(), [Free, Presented, Free, Free, Free], "showing frame 1 now would go back in time");
    assert_eq!(ring.acquire().unwrap(), Shown { slot: 1, generation: 0, seq: 2, is_new: false, more: false });
    assert_eq!((ring.stats().completed, ring.stats().dropped), (2, 1));
}

#[test]
fn reset_frees_every_slot_and_makes_old_tickets_stale() {
    let ring = Ring::new();
    finished(&ring);
    ring.acquire();
    finished(&ring); // waiting, discarded by the reset
    let late = ring.reserve().unwrap();
    ring.reset();
    assert_eq!(ring.states(), [Free; SLOTS]);
    assert_eq!(ring.generation(), 1);
    assert_eq!(ring.acquire(), None, "nothing to show from the new textures yet");

    // The old frame is still on the GPU, so it still counts as in flight.
    assert_eq!(ring.in_flight(), 1);
    let fresh = ring.reserve().unwrap();
    assert_eq!((fresh.slot(), fresh.generation()), (0, 1));
    assert_eq!(ring.reserve(), None);

    let before = ring.states();
    assert_eq!(ring.complete(late), Err(RingError::Stale { ticket_generation: 0, generation: 1 }));
    assert_eq!(ring.states(), before, "a stale ticket changes no slot");
    assert_eq!(ring.in_flight(), 1);
    let stats = ring.stats();
    assert_eq!((stats.stale, stats.discarded), (1, 1));
    ring.complete(fresh).unwrap();
}

#[test]
fn cancelling_a_stale_ticket_is_stale_too() {
    let ring = Ring::new();
    let ticket = ring.reserve().unwrap();
    ring.reset();
    assert_eq!(ring.cancel(ticket), Err(RingError::Stale { ticket_generation: 0, generation: 1 }));
    assert_eq!((ring.in_flight(), ring.stats().stale), (0, 1));
}

#[test]
fn a_ticket_the_ring_did_not_hand_out_is_refused_and_changes_nothing() {
    let ring = Ring::new();
    let real = ring.reserve().unwrap();
    let forged = [
        Ticket { slot: 3, generation: 0, seq: 1 },  // a Free slot
        Ticket { slot: 0, generation: 0, seq: 7 },  // the right slot, another frame
        Ticket { slot: 99, generation: 0, seq: 1 }, // no such slot
        Ticket { slot: 0, generation: 5, seq: 1 },  // a generation that never was
    ];
    for ticket in forged {
        let (slot, seq) = (ticket.slot, ticket.seq);
        let before = (ring.states(), ring.in_flight(), ring.stats());
        assert_eq!(ring.complete(ticket), Err(RingError::UnknownTicket { slot, seq }));
        assert_eq!((ring.states(), ring.in_flight(), ring.stats()), before);
    }
    ring.complete(real).unwrap();
}

/// xorshift64*: enough randomness for shuffling operations.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }
}

/// Random operations in a random order, with the invariants checked after
/// every one (debug builds check them inside the ring too) and the two
/// promises to the platform checked from the outside: a reserved slot is
/// never the shown one or the one shown before it, and frames are shown in
/// order.
#[test]
fn random_operations_keep_every_promise() {
    let ring = Ring::new();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut out: Vec<Ticket> = Vec::new();
    let (mut showing, mut previous): (Option<usize>, Option<usize>) = (None, None);
    let mut last_seq = 0;
    for _ in 0..200_000 {
        match rng.below(100) {
            0..30 => {
                if let Some(ticket) = ring.reserve() {
                    assert!(Some(ticket.slot()) != showing && Some(ticket.slot()) != previous);
                    out.push(ticket);
                } else {
                    assert_eq!(ring.in_flight(), MAX_IN_FLIGHT);
                }
            }
            30..55 if !out.is_empty() => {
                let ticket = out.swap_remove(rng.below(out.len() as u64) as usize);
                let current = ticket.generation() == ring.generation();
                assert_eq!(ring.complete(ticket).is_ok(), current);
            }
            55..58 if !out.is_empty() => {
                let ticket = out.swap_remove(rng.below(out.len() as u64) as usize);
                let current = ticket.generation() == ring.generation();
                assert_eq!(ring.cancel(ticket).is_ok(), current);
            }
            58..99 => {
                if let Some(shown) = ring.acquire() {
                    if shown.is_new {
                        assert!(shown.seq > last_seq, "frame {} shown after {last_seq}", shown.seq);
                        last_seq = shown.seq;
                        previous = showing.filter(|&s| s != shown.slot);
                        showing = Some(shown.slot);
                    } else {
                        assert_eq!(Some(shown.slot), showing);
                    }
                }
            }
            99 => {
                ring.reset();
                (showing, previous) = (None, None);
            }
            _ => {}
        }
        ring.lock().check().unwrap();
    }
    for ticket in out {
        let _ = ring.complete(ticket);
    }
    let stats = ring.stats();
    assert_eq!(ring.in_flight(), 0);
    assert_eq!(stats.reserved, stats.completed + stats.cancelled + stats.stale);
    assert!(stats.presented > 10_000 && stats.dropped > 0 && stats.stale > 0, "{stats:?}");
}
