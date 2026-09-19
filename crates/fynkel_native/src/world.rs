//! Minimal world for the ABI v0 round trip: generational entity slots with a
//! transform each. No hecs in M0 (ADR-0002); the point is the bridge
//! contract — staging, tick application, subscriptions, snapshot/event/
//! diagnostic buffers — not the ECS.

use std::collections::HashSet;

use crate::wire::{self, BufferKind, BufferWriter, Command, CommandKind, Transform};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    Simulation = 0,
    Presentation = 1,
}

struct Slot {
    /// Starts at 1 so that handle 0 (slot 0, generation 0) is never valid.
    generation: u32,
    alive: bool,
    transform: Transform,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            generation: 1,
            alive: false,
            transform: Transform::IDENTITY,
        }
    }
}

pub struct StepOutcome {
    pub tick: u64,
    pub applied: u32,
    pub rejected: u32,
    pub snapshot_records: u32,
    pub event_records: u32,
    pub diagnostic_records: u32,
    pub live_entities: u32,
}

struct Diagnostic {
    command_index: u32,
    code: u32,
    entity: u64,
    component: u32,
    lane: u32,
    message: String,
}

pub struct World {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: u32,
    tick: u64,
    /// Simulation-lane commands waiting for the next tick, with their index
    /// in the batch they came from (for diagnostics).
    staged: Vec<(u32, Command)>,
    subscribed_transforms: HashSet<u64>,
    events: Vec<(u32, u64, u64, u64)>,
    diagnostics: Vec<Diagnostic>,
    snapshot_buf: Vec<u8>,
    events_buf: Vec<u8>,
    diagnostics_buf: Vec<u8>,
    applied_this_tick: u32,
    pub poisoned: bool,
}

pub fn handle(slot: u32, generation: u32) -> u64 {
    ((generation as u64) << 32) | slot as u64
}

impl World {
    pub fn new(initial_capacity: u32) -> Self {
        let mut w = Self {
            slots: Vec::with_capacity(initial_capacity as usize),
            free: Vec::new(),
            live: 0,
            tick: 0,
            staged: Vec::new(),
            subscribed_transforms: HashSet::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
            snapshot_buf: Vec::new(),
            events_buf: Vec::new(),
            diagnostics_buf: Vec::new(),
            applied_this_tick: 0,
            poisoned: false,
        };
        // Empty but valid buffers from the start.
        w.snapshot_buf = BufferWriter::begin(Vec::new(), BufferKind::Snapshot).finish();
        w.events_buf = BufferWriter::begin(Vec::new(), BufferKind::Events).finish();
        w.diagnostics_buf = BufferWriter::begin(Vec::new(), BufferKind::Diagnostics).finish();
        w
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn snapshot(&self) -> &[u8] {
        &self.snapshot_buf
    }
    pub fn events(&self) -> &[u8] {
        &self.events_buf
    }
    pub fn diagnostics(&self) -> &[u8] {
        &self.diagnostics_buf
    }

    fn resolve(&self, entity: u64) -> Option<u32> {
        let slot = (entity & 0xffff_ffff) as u32;
        let generation = (entity >> 32) as u32;
        let s = self.slots.get(slot as usize)?;
        (s.alive && s.generation == generation).then_some(slot)
    }

    pub fn transform(&self, entity: u64) -> Option<Transform> {
        self.resolve(entity)
            .map(|i| self.slots[i as usize].transform)
    }

    /// Validates the buffer and stages (simulation) or applies (presentation).
    pub fn submit(&mut self, lane: Lane, buf: &[u8]) -> Result<bool, wire::ProtocolError> {
        let (count, body) = wire::validate(buf, BufferKind::Commands, wire::COMMAND_SIZE)?;
        let mut all_ok = true;
        for i in 0..count as usize {
            let cmd = Command::read(&body[i * wire::COMMAND_SIZE..(i + 1) * wire::COMMAND_SIZE]);
            match lane {
                Lane::Simulation => self.staged.push((i as u32, cmd)),
                Lane::Presentation => {
                    if !self.apply(i as u32, Lane::Presentation, &cmd) {
                        all_ok = false;
                    }
                }
            }
        }
        Ok(all_ok)
    }

    fn reject(&mut self, index: u32, lane: Lane, code: u32, entity: u64, msg: String) -> bool {
        self.diagnostics.push(Diagnostic {
            command_index: index,
            code,
            entity,
            component: if code == wire::DIAG_INVALID_PAYLOAD {
                wire::COMPONENT_TRANSFORM
            } else {
                0
            },
            lane: lane as u32,
            message: msg,
        });
        false
    }

    fn apply(&mut self, index: u32, lane: Lane, cmd: &Command) -> bool {
        let Some(kind) = CommandKind::from_u32(cmd.kind) else {
            return self.reject(
                index,
                lane,
                wire::DIAG_UNKNOWN_COMMAND,
                0,
                format!("unknown command kind {}", cmd.kind),
            );
        };
        let ok = match kind {
            CommandKind::Spawn => {
                if cmd.target == 0 {
                    return self.reject(
                        index,
                        lane,
                        wire::DIAG_INVALID_TEMP_ID,
                        0,
                        "spawn temp id must be non-zero".into(),
                    );
                }
                let t = Transform::read(&cmd.payload[..wire::TRANSFORM_SIZE]);
                if !t.is_finite() {
                    return self.reject(
                        index,
                        lane,
                        wire::DIAG_INVALID_PAYLOAD,
                        0,
                        "transform is not finite".into(),
                    );
                }
                let slot = match self.free.pop() {
                    Some(s) => s,
                    None => {
                        self.slots.push(Slot::default());
                        (self.slots.len() - 1) as u32
                    }
                };
                let s = &mut self.slots[slot as usize];
                s.alive = true;
                s.transform = t;
                self.live += 1;
                let h = handle(slot, s.generation);
                self.events.push((wire::EVENT_SPAWNED, cmd.target, h, 0));
                true
            }
            CommandKind::Destroy => match self.resolve(cmd.target) {
                Some(slot) => {
                    let s = &mut self.slots[slot as usize];
                    s.alive = false;
                    s.generation = s.generation.wrapping_add(1);
                    self.free.push(slot);
                    self.live -= 1;
                    self.subscribed_transforms.remove(&cmd.target);
                    self.events.push((wire::EVENT_DESTROYED, cmd.target, 0, 0));
                    true
                }
                None => {
                    return self.reject(
                        index,
                        lane,
                        wire::DIAG_STALE_ENTITY,
                        cmd.target,
                        "destroy: stale or unknown entity".into(),
                    )
                }
            },
            CommandKind::SetTransform => {
                let t = Transform::read(&cmd.payload[..wire::TRANSFORM_SIZE]);
                if !t.is_finite() {
                    return self.reject(
                        index,
                        lane,
                        wire::DIAG_INVALID_PAYLOAD,
                        cmd.target,
                        "transform is not finite".into(),
                    );
                }
                match self.resolve(cmd.target) {
                    Some(slot) => {
                        self.slots[slot as usize].transform = t;
                        true
                    }
                    None => {
                        return self.reject(
                            index,
                            lane,
                            wire::DIAG_STALE_ENTITY,
                            cmd.target,
                            "set_transform: stale or unknown entity".into(),
                        )
                    }
                }
            }
        };
        if ok {
            self.applied_this_tick += 1;
        }
        ok
    }

    pub fn subscribe(&mut self, entities: &[u64], enable: bool) {
        for &e in entities {
            if enable {
                self.subscribed_transforms.insert(e);
            } else {
                self.subscribed_transforms.remove(&e);
            }
        }
    }

    /// Applies staged simulation commands, advances the tick, rebuilds the
    /// outgoing buffers. `dt` is unused by this toy world but part of the
    /// contract (fixed step, blueprint §5.1).
    pub fn step(&mut self, _dt: f32) -> StepOutcome {
        let staged = std::mem::take(&mut self.staged);
        for (index, cmd) in &staged {
            self.apply(*index, Lane::Simulation, cmd);
        }
        self.tick += 1;

        // Snapshot: subscribed, live transforms, in handle order for
        // determinism of the buffer layout.
        let mut subs: Vec<u64> = self.subscribed_transforms.iter().copied().collect();
        subs.sort_unstable();
        let mut w =
            BufferWriter::begin(std::mem::take(&mut self.snapshot_buf), BufferKind::Snapshot);
        for e in subs {
            if let Some(t) = self.transform(e) {
                let r = w.record();
                r.extend_from_slice(&e.to_le_bytes());
                t.write(r);
            }
        }
        let snapshot_records = w.count();
        self.snapshot_buf = w.finish();

        let tick_lo = self.tick as u32;
        let mut w = BufferWriter::begin(std::mem::take(&mut self.events_buf), BufferKind::Events);
        for (kind, a, b, c) in self.events.drain(..) {
            let r = w.record();
            r.extend_from_slice(&kind.to_le_bytes());
            r.extend_from_slice(&tick_lo.to_le_bytes());
            r.extend_from_slice(&a.to_le_bytes());
            r.extend_from_slice(&b.to_le_bytes());
            r.extend_from_slice(&c.to_le_bytes());
        }
        let event_records = w.count();
        self.events_buf = w.finish();

        // Diagnostics: fixed records, then the message bytes.
        let diags = std::mem::take(&mut self.diagnostics);
        let mut w = BufferWriter::begin(
            std::mem::take(&mut self.diagnostics_buf),
            BufferKind::Diagnostics,
        );
        let mut msg_off = wire::HEADER_SIZE + diags.len() * wire::DIAGNOSTIC_SIZE;
        for d in &diags {
            let r = w.record();
            r.extend_from_slice(&d.command_index.to_le_bytes());
            r.extend_from_slice(&d.code.to_le_bytes());
            r.extend_from_slice(&d.entity.to_le_bytes());
            r.extend_from_slice(&d.component.to_le_bytes());
            r.extend_from_slice(&(msg_off as u32).to_le_bytes());
            r.extend_from_slice(&(d.message.len() as u32).to_le_bytes());
            r.extend_from_slice(&d.lane.to_le_bytes());
            msg_off += d.message.len();
        }
        for d in &diags {
            w.bytes.extend_from_slice(d.message.as_bytes());
        }
        let diagnostic_records = w.count();
        self.diagnostics_buf = w.finish();

        StepOutcome {
            tick: self.tick,
            applied: std::mem::take(&mut self.applied_this_tick),
            rejected: diagnostic_records,
            snapshot_records,
            event_records,
            diagnostic_records,
            live_entities: self.live,
        }
    }
}
