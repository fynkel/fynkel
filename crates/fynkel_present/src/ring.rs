//! The frame ring: the slots the render worker draws into and the
//! presenter (Flutter's raster thread, through the Swift plugin) shows.
//!
//! This is only the bookkeeping. A slot is an index; the platform owns the
//! textures behind the indices (IOSurfaces on Apple). Stage 1 plan §3.5.
//!
//! ```text
//!          reserve           complete           acquire            acquire (a newer frame)
//!   Free ──────────► Rendering ──────► Ready ──────────► Presented ──────────► Retiring
//!    ▲                   │ cancel        │ dropped                                │
//!    └───────────────────┴───────────────┴────────────────────────────────────────┘
//! ```
//!
//! - At most [`MAX_IN_FLIGHT`] frames are on the GPU. [`Ring::reserve`]
//!   says no to a third, and the worker coalesces scenes until one finishes.
//! - [`Ring::acquire`] shows the **oldest** finished frame, keeping at most
//!   [`MAX_QUEUED`] waiting (older ones are dropped), and says when another
//!   is waiting so the host asks for one more frame. A frame that finishes
//!   just after a vsync is then shown on the next one instead of being
//!   replaced by its successor.
//! - The slot being shown and the one shown before it are never drawn
//!   into: the compositor may still be reading the previous one.
//! - Without a Free slot, `reserve` reuses the oldest Ready one (that frame
//!   is dropped): the presenter has fallen behind, and newer frames win.
//! - Frames are shown in order. One that finishes after a newer frame is
//!   already on screen is dropped.
//!
//! Every move between states goes through one checked table
//! ([`SlotState::can_become`]). Debug builds also check the ring's
//! invariants after every operation, including that every frame reserved is
//! accounted for.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// How many slots (textures) the ring has.
pub const SLOTS: usize = 5;
/// How many frames may be on the GPU at once.
pub const MAX_IN_FLIGHT: usize = 2;
/// How many finished frames may wait for the presenter, counting the one
/// it takes next.
pub const MAX_QUEUED: usize = 2;

// The shown slot, the previous one and every frame in flight still leave a
// slot for a finished frame to wait in. That also means `reserve` always
// finds a slot while fewer than MAX_IN_FLIGHT frames are in flight.
const _: () = assert!(SLOTS > 2 + MAX_IN_FLIGHT);
const _: () = assert!(MAX_IN_FLIGHT >= 1 && MAX_QUEUED >= 1);

/// Where a slot is in its cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotState {
    /// Nobody uses it.
    Free,
    /// A frame is being drawn into it.
    Rendering,
    /// Its frame is finished and waits to be shown.
    Ready,
    /// It is on screen.
    Presented,
    /// It was on screen until the latest frame; the compositor may still
    /// read it.
    Retiring,
}

impl SlotState {
    /// Every state, for tables.
    pub const ALL: [SlotState; 5] =
        [SlotState::Free, SlotState::Rendering, SlotState::Ready, SlotState::Presented, SlotState::Retiring];

    /// Whether a slot may move from `self` to `next`. These are the only
    /// moves the ring makes ([`Ring::reset`] aside, which starts a new set
    /// of slots):
    ///
    /// | move                 | made by                                          |
    /// |----------------------|--------------------------------------------------|
    /// | Free → Rendering     | `reserve`                                        |
    /// | Rendering → Ready    | `complete`                                       |
    /// | Rendering → Free     | `cancel`                                         |
    /// | Ready → Presented    | `acquire`                                        |
    /// | Ready → Free         | dropped: too many waiting, too late, or the slot is needed |
    /// | Presented → Retiring | `acquire`, when a newer frame is shown           |
    /// | Retiring → Free      | `acquire`, when the one after that is shown      |
    pub const fn can_become(self, next: SlotState) -> bool {
        use SlotState::*;
        matches!(
            (self, next),
            (Free, Rendering)
                | (Rendering, Ready)
                | (Rendering, Free)
                | (Ready, Presented)
                | (Ready, Free)
                | (Presented, Retiring)
                | (Retiring, Free)
        )
    }
}

/// A slot handed to one frame by [`Ring::reserve`]. Give it back exactly
/// once, with [`Ring::complete`] or [`Ring::cancel`]; both take it by value,
/// so a second time doesn't compile.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a ticket must be completed or cancelled, or the ring loses a slot"]
pub struct Ticket {
    slot: usize,
    generation: u64,
    seq: u64,
}

impl Ticket {
    /// Which texture to draw into.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The ring's generation when the slot was reserved; see [`Ring::reset`].
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The frame's number. Frames are numbered from 1 in reserve order.
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// What [`Ring::acquire`] gives the presenter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The slot to show.
    pub slot: usize,
    /// The generation the slot belongs to; see [`Ring::reset`]. A platform
    /// that just made new textures can tell an index into the old set from
    /// one into the new.
    pub generation: u64,
    /// Its frame's number.
    pub seq: u64,
    /// `false` when no new frame was finished and this is the one already
    /// on screen.
    pub is_new: bool,
    /// Another finished frame is waiting: ask the host for one more frame.
    pub more: bool,
}

/// What went wrong. None of these changes any slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingError {
    /// The ring was [reset](Ring::reset) after this ticket was handed out.
    /// Not a bug: the frame was drawn into a texture that is gone, and is
    /// discarded. Its in-flight place is given back.
    Stale { ticket_generation: u64, generation: u64 },
    /// A move outside [`SlotState::can_become`]. The public API can't cause
    /// one; it would mean a bug in the ring, and nothing is changed.
    Illegal { slot: usize, from: SlotState, to: SlotState },
    /// The slot doesn't hold this ticket's frame. Only a ticket the ring
    /// didn't hand out can cause it.
    UnknownTicket { slot: usize, seq: u64 },
}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RingError::Stale { ticket_generation, generation } => {
                write!(f, "ticket from generation {ticket_generation}, the ring is at {generation}")
            }
            RingError::Illegal { slot, from, to } => write!(f, "slot {slot} can't move from {from:?} to {to:?}"),
            RingError::UnknownTicket { slot, seq } => write!(f, "slot {slot} doesn't hold frame {seq}"),
        }
    }
}

impl std::error::Error for RingError {}

/// Counts since the ring was made. Every frame reserved ends up in exactly
/// one of `completed`, `cancelled` or `stale` (or is still in flight), and
/// every frame completed in exactly one of `presented`, `dropped` or
/// `discarded` (or still waits).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RingStats {
    pub reserved: u64,
    pub completed: u64,
    pub cancelled: u64,
    /// Completed or cancelled after a reset.
    pub stale: u64,
    pub presented: u64,
    /// Finished but never shown: more than [`MAX_QUEUED`] were waiting, or
    /// `reserve` needed the slot.
    pub dropped: u64,
    /// Finished, waiting and thrown away by a reset.
    pub discarded: u64,
}

/// The ring. All methods take `&self`; share it between the render worker,
/// the GPU's completion callback and the presenter.
#[derive(Debug)]
pub struct Ring {
    slots: Mutex<Slots>,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

impl Ring {
    /// A ring with every slot Free, at generation 0.
    pub fn new() -> Self {
        let slots = Slots {
            state: [SlotState::Free; SLOTS],
            seq: [0; SLOTS],
            freed_at: [0; SLOTS],
            clock: 0,
            generation: 0,
            next_seq: 1,
            in_flight: 0,
            stale_in_flight: 0,
            stats: RingStats::default(),
        };
        Self { slots: Mutex::new(slots) }
    }

    /// A slot for the next frame, or `None` while [`MAX_IN_FLIGHT`] frames
    /// are already in flight.
    ///
    /// It is the slot that has been Free the longest, so every slot takes
    /// its turn and the one the compositor read last is reused last. With
    /// no Free slot, the oldest Ready one is reused and its frame dropped.
    pub fn reserve(&self) -> Option<Ticket> {
        self.with(Slots::reserve)
    }

    /// The frame's GPU work finished: its slot becomes Ready. A frame older
    /// than the one on screen is dropped instead, so frames are never shown
    /// out of order.
    pub fn complete(&self, ticket: Ticket) -> Result<(), RingError> {
        self.with(|slots| slots.give_back(ticket, SlotState::Ready))
    }

    /// The frame never reached the GPU: its slot becomes Free again.
    pub fn cancel(&self, ticket: Ticket) -> Result<(), RingError> {
        self.with(|slots| slots.give_back(ticket, SlotState::Free))
    }

    /// The slot to show now (the presenter calls this once per raster), or
    /// `None` before the first frame. Allocates nothing.
    pub fn acquire(&self) -> Option<Shown> {
        self.with(Slots::acquire)
    }

    /// Starts a new set of slots (the platform made new textures, say for a
    /// new size): every slot becomes Free and the generation goes up.
    /// Tickets handed out before are now stale, but still count as in
    /// flight until they come back, so the GPU never has more than
    /// [`MAX_IN_FLIGHT`] frames even across a reset.
    pub fn reset(&self) {
        self.with(Slots::reset)
    }

    /// The current generation.
    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Frames handed out and not yet given back.
    pub fn in_flight(&self) -> usize {
        self.lock().in_flight
    }

    /// Every slot's state.
    pub fn states(&self) -> [SlotState; SLOTS] {
        self.lock().state
    }

    pub fn stats(&self) -> RingStats {
        self.lock().stats
    }

    fn lock(&self) -> MutexGuard<'_, Slots> {
        // No operation panics halfway, so a poisoned lock still guards
        // consistent slots.
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs one operation, and in debug builds checks every invariant
    /// after it.
    fn with<R>(&self, op: impl FnOnce(&mut Slots) -> R) -> R {
        let mut slots = self.lock();
        let result = op(&mut slots);
        #[cfg(debug_assertions)]
        if let Err(broken) = slots.check() {
            panic!("frame ring: {broken}\n{slots:?}");
        }
        result
    }
}

#[derive(Debug)]
struct Slots {
    state: [SlotState; SLOTS],
    /// The frame number in each slot (0 while Free).
    seq: [u64; SLOTS],
    /// When each slot last became Free, on `clock`.
    freed_at: [u64; SLOTS],
    clock: u64,
    generation: u64,
    next_seq: u64,
    /// Tickets handed out and not given back, from any generation.
    in_flight: usize,
    /// The part of `in_flight` from earlier generations.
    stale_in_flight: usize,
    stats: RingStats,
}

impl Slots {
    /// The one place a slot changes state (resets aside).
    fn advance(&mut self, slot: usize, to: SlotState) -> Result<(), RingError> {
        let from = self.state[slot];
        if !from.can_become(to) {
            return Err(RingError::Illegal { slot, from, to });
        }
        self.state[slot] = to;
        if to == SlotState::Free {
            self.seq[slot] = 0;
            self.clock += 1;
            self.freed_at[slot] = self.clock;
        }
        Ok(())
    }

    fn reserve(&mut self) -> Option<Ticket> {
        if self.in_flight >= MAX_IN_FLIGHT {
            return None;
        }
        let free = (0..SLOTS).filter(|&i| self.state[i] == SlotState::Free).min_by_key(|&i| self.freed_at[i]);
        let slot = match free {
            Some(slot) => slot,
            None => {
                // The presenter has fallen behind; the oldest waiting frame
                // gives way. There always is one: see the assertion on SLOTS.
                let slot = self.oldest(SlotState::Ready)?;
                self.drop_frame(slot).ok()?;
                slot
            }
        };
        self.advance(slot, SlotState::Rendering).ok()?;
        let seq = self.next_seq;
        self.next_seq += 1;
        self.seq[slot] = seq;
        self.in_flight += 1;
        self.stats.reserved += 1;
        Some(Ticket { slot, generation: self.generation, seq })
    }

    /// `to` is Ready (complete) or Free (cancel).
    fn give_back(&mut self, ticket: Ticket, to: SlotState) -> Result<(), RingError> {
        let Ticket { slot, generation, seq } = ticket;
        let unknown = RingError::UnknownTicket { slot, seq };
        if generation != self.generation {
            if generation > self.generation || self.stale_in_flight == 0 {
                return Err(unknown);
            }
            self.stale_in_flight -= 1;
            self.in_flight -= 1;
            self.stats.stale += 1;
            return Err(RingError::Stale { ticket_generation: generation, generation: self.generation });
        }
        if slot >= SLOTS || self.state[slot] != SlotState::Rendering || self.seq[slot] != seq {
            return Err(unknown);
        }
        self.advance(slot, to)?;
        self.in_flight -= 1;
        if to == SlotState::Free {
            self.stats.cancelled += 1;
            return Ok(());
        }
        self.stats.completed += 1;
        let on_screen = self.find(SlotState::Presented).map_or(0, |i| self.seq[i]);
        if seq < on_screen {
            self.drop_frame(slot)?;
        }
        Ok(())
    }

    fn acquire(&mut self) -> Option<Shown> {
        let mut waiting = self.count(SlotState::Ready);
        while waiting > MAX_QUEUED {
            let oldest = self.oldest(SlotState::Ready)?;
            self.drop_frame(oldest).ok()?;
            waiting -= 1;
        }
        let current = self.find(SlotState::Presented);
        let Some(next) = self.oldest(SlotState::Ready) else {
            return current.map(|slot| Shown {
                slot,
                generation: self.generation,
                seq: self.seq[slot],
                is_new: false,
                more: false,
            });
        };
        // The order matters: the previous slot is freed before the current
        // one takes its place.
        if let Some(previous) = self.find(SlotState::Retiring) {
            self.advance(previous, SlotState::Free).ok()?;
        }
        if let Some(current) = current {
            self.advance(current, SlotState::Retiring).ok()?;
        }
        self.advance(next, SlotState::Presented).ok()?;
        self.stats.presented += 1;
        Some(Shown { slot: next, generation: self.generation, seq: self.seq[next], is_new: true, more: waiting > 1 })
    }

    fn reset(&mut self) {
        self.stats.discarded += self.count(SlotState::Ready) as u64;
        self.state = [SlotState::Free; SLOTS];
        self.seq = [0; SLOTS];
        self.freed_at = [0; SLOTS];
        self.generation += 1;
        self.stale_in_flight = self.in_flight;
    }

    fn drop_frame(&mut self, slot: usize) -> Result<(), RingError> {
        self.advance(slot, SlotState::Free)?;
        self.stats.dropped += 1;
        Ok(())
    }

    fn find(&self, state: SlotState) -> Option<usize> {
        self.state.iter().position(|&s| s == state)
    }

    fn count(&self, state: SlotState) -> usize {
        self.state.iter().filter(|&&s| s == state).count()
    }

    /// The slot in `state` holding the oldest frame.
    fn oldest(&self, state: SlotState) -> Option<usize> {
        (0..SLOTS).filter(|&i| self.state[i] == state).min_by_key(|&i| self.seq[i])
    }

    /// Every invariant, or the first one broken.
    #[cfg(any(debug_assertions, test))]
    fn check(&self) -> Result<(), String> {
        use SlotState::*;
        let [free, rendering, ready, presented, retiring] = SlotState::ALL.map(|s| self.count(s));
        let _ = free;
        let current_in_flight = self.in_flight.checked_sub(self.stale_in_flight).ok_or("more stale than in flight")?;
        if self.in_flight > MAX_IN_FLIGHT {
            return Err(format!("{} frames in flight", self.in_flight));
        }
        if rendering != current_in_flight {
            return Err(format!("{rendering} slots Rendering, {current_in_flight} tickets out"));
        }
        if presented > 1 || retiring > presented {
            return Err(format!("{presented} Presented, {retiring} Retiring"));
        }
        for i in 0..SLOTS {
            let (state, seq) = (self.state[i], self.seq[i]);
            if (state == Free) != (seq == 0) || seq >= self.next_seq {
                return Err(format!("slot {i} is {state:?} with frame {seq}"));
            }
            if seq != 0 && (0..i).any(|j| self.seq[j] == seq) {
                return Err(format!("frame {seq} is in two slots"));
            }
        }
        let seq_of = |state| self.find(state).map(|i| self.seq[i]);
        if let (Some(old), Some(shown)) = (seq_of(Retiring), seq_of(Presented))
            && old >= shown
        {
            return Err(format!("frame {old} retiring behind frame {shown}"));
        }
        if let Some(shown) = seq_of(Presented)
            && let Some(i) = (0..SLOTS).find(|&i| self.state[i] == Ready && self.seq[i] < shown)
        {
            return Err(format!("frame {} waits behind frame {shown}, already shown", self.seq[i]));
        }
        let s = &self.stats;
        if s.reserved != self.next_seq - 1 || s.reserved != s.completed + s.cancelled + s.stale + self.in_flight as u64
        {
            return Err(format!("reserved frames unaccounted for: {s:?}, {} in flight", self.in_flight));
        }
        if s.completed != s.presented + s.dropped + s.discarded + ready as u64 {
            return Err(format!("finished frames unaccounted for: {s:?}, {ready} Ready"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
