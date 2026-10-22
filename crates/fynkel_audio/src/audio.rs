//! The game's half (docs/stage-11.md §3.1): it decides which voice plays
//! what, which is stolen when all are busy, and when, and tells the
//! mixer. Until a device opens (and in tests, where none does) it renders
//! the mixer itself, as much as the frames' time, so sounds still end when
//! they would.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::clip::Clip;
use crate::device::Device;
use crate::mixer::{COMMANDS, Command, Ended, MAX_GROUPS, MAX_VOICES, Mixer, RETURNS, Settings, Shared};
use crate::session::{self, Session};
use crate::spatial::{self, Listener, Mix, Spatial};
use crate::stream::{self, Streamer};

/// The rate and channels sound is made at without a device.
pub const QUIET_RATE: u32 = 48_000;

/// The least important priority, as Unity's: 0 is the most.
pub const LOWEST_PRIORITY: u16 = 256;

/// The highest pitch, 8 times the clip's speed.
pub const MAX_PITCH: f32 = 8.0;

/// The lowest pitch.
pub const MIN_PITCH: f32 = 0.05;

/// The longest fade, in seconds.
pub const MAX_FADE: f32 = 3_600.0;

/// How much is rendered at once without a device: streams are decoded
/// between, as a device's thread would.
const CHUNK: usize = 1024;

/// How often a phone's call is looked for, with a device.
const CALL_CHECK: Duration = Duration::from_millis(500);

/// A playing voice, for as long as it plays: its slot and which use of
/// that slot it is. Never 0.
pub type Handle = u64;

/// How to play a clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    pub settings: Settings,
    /// 0 (kept longest) to 256 (stolen first).
    pub priority: u16,
    /// Falls silent and holds while the game is paused.
    pub pausable: bool,
    /// Whose it is (an `AudioSource`'s id, say), to stop all of one
    /// owner's at once; 0 for no one's.
    pub owner: u64,
    /// Where in the clip to start, in seconds.
    pub from: f64,
    /// 2D or 3D, and how it fades.
    pub spatial: Spatial,
    /// Where it is, for a 3D sound; see [`Audio::place`].
    pub position: [f32; 3],
    /// Seconds it fades in over, from silence; 0 for none.
    pub fade_in: f32,
}

impl Default for Play {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            priority: 128,
            pausable: true,
            owner: 0,
            from: 0.0,
            spatial: Spatial::default(),
            position: [0.0; 3],
            fade_in: 0.0,
        }
    }
}

/// A voice that stopped playing without being told to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped {
    pub handle: Handle,
    pub owner: u64,
    /// It was stolen for a more important sound, rather than reaching
    /// the end of its clip.
    pub stolen: bool,
}

/// Counts, for the game and its tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Whether a device plays the sound.
    pub device: bool,
    pub rate: u32,
    pub channels: u32,
    /// Voices the mixer has now, paused ones too, as of its last callback.
    pub voices: u32,
    pub started: u64,
    /// Voices stolen for more important ones.
    pub stolen: u64,
    /// Sounds not played: every voice was more important.
    pub refused: u64,
    pub callbacks: u64,
    /// Times the device wasn't given its sound in time.
    pub underruns: u64,
    /// Times the device was lost and opened again.
    pub reopened: u64,
    /// Callbacks in which a streamed clip's voice ran out of decoded sound
    /// (a gap): never, while its decoder keeps up.
    pub starved: u64,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    generation: u32,
    /// The mixer has it: from its start until its end comes back.
    busy: bool,
    /// Told to stop: it isn't playing, but the mixer still fades it.
    stopped: bool,
    priority: u16,
    owner: u64,
    /// When it started, to steal the oldest of equals.
    order: u64,
    rate: u32,
    spatial: Spatial,
    position: [f32; 3],
    /// The mix the mixer was last sent.
    mix: Mix,
}

impl Slot {
    const FREE: Self = Self {
        generation: 0,
        busy: false,
        stopped: false,
        priority: 0,
        owner: 0,
        order: 0,
        rate: 1,
        spatial: Spatial {
            blend: 0.0,
            pan: 0.0,
            min_distance: 1.0,
            max_distance: 500.0,
            rolloff: spatial::Rolloff::Logarithmic,
        },
        position: [0.0; 3],
        mix: Mix::STRAIGHT,
    };
}

/// The game's side of the sound.
#[derive(Debug)]
pub struct Audio {
    slots: [Slot; MAX_VOICES],
    order: u64,
    commands: rtrb::Producer<Command>,
    returns: rtrb::Consumer<Ended>,
    shared: Arc<Shared>,
    mixer: Arc<Mutex<Mixer>>,
    streamer: Streamer,
    session: Session,
    listener: Listener,
    /// The listener or a 3D voice moved since the last update.
    moved: bool,
    device: Option<Device>,
    /// The device was asked for: opened again when it's lost.
    wants_device: bool,
    suspended: bool,
    /// A phone call is on: the device waits for it, as when suspended.
    in_call: bool,
    last_call_check: Option<Instant>,
    last_try: Option<Instant>,
    stopped: Vec<Stopped>,
    /// Without a device: frames of time not yet rendered, and where.
    owed: f64,
    scratch: Vec<f32>,
    stats: Stats,
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

impl Audio {
    /// Sound without a device, at [`QUIET_RATE`] in stereo, until
    /// [`Audio::open_device`].
    pub fn new() -> Self {
        let (commands, take) = rtrb::RingBuffer::new(COMMANDS);
        let (give, returns) = rtrb::RingBuffer::new(RETURNS);
        let shared = Arc::new(Shared::default());
        let mixer = Arc::new(Mutex::new(Mixer::new(QUIET_RATE, 2, take, give, shared.clone())));
        // Some systems make a lock's insides the first time it's taken:
        // here, not on the audio thread.
        drop(mixer.try_lock());
        Self {
            slots: [Slot::FREE; MAX_VOICES],
            order: 0,
            commands,
            returns,
            shared,
            mixer,
            streamer: Streamer::default(),
            session: Session::default(),
            listener: Listener::default(),
            moved: false,
            device: None,
            wants_device: false,
            suspended: false,
            in_call: false,
            last_call_check: None,
            last_try: None,
            // A steal per command, and an end per return, at most.
            stopped: Vec::with_capacity(COMMANDS + RETURNS),
            owed: 0.0,
            scratch: vec![0.0; CHUNK * 2],
            stats: Stats::default(),
        }
    }

    /// Plays through the default output from now on, and again after it's
    /// lost. Without one (or when it can't be opened), sound goes on
    /// without it, and the error says why.
    pub fn open_device(&mut self) -> Result<(), String> {
        self.wants_device = true;
        if self.device.is_some() {
            return Ok(());
        }
        self.try_device()
    }

    fn try_device(&mut self) -> Result<(), String> {
        self.last_try = Some(Instant::now());
        session::apply(self.session)?;
        self.streamer.pump();
        self.streamer.start()?;
        let device = match Device::open(&self.mixer) {
            Ok(device) => device,
            Err(e) => {
                self.streamer.stop();
                return Err(e);
            }
        };
        if self.suspended || self.in_call {
            device.suspend(true)?;
        }
        self.device = Some(device);
        self.owed = 0.0;
        Ok(())
    }

    /// Closes the device: sound goes on without one.
    pub fn close_device(&mut self) {
        self.wants_device = false;
        self.drop_device();
    }

    fn drop_device(&mut self) {
        if let Some(device) = self.device.take() {
            self.stats.underruns += device.underruns.load(Ordering::Relaxed);
            drop(device);
            self.streamer.stop();
            if let Ok(mut mixer) = self.mixer.lock() {
                mixer.set_output(QUIET_RATE, 2);
            }
        }
    }

    /// While the app is away: the device stops, and so does the sound.
    pub fn suspend(&mut self, suspended: bool) -> Result<(), String> {
        self.suspended = suspended;
        match &self.device {
            Some(device) => device.suspend(suspended || self.in_call),
            None => Ok(()),
        }
    }

    /// How the game's sound sits with the phone's (iOS).
    pub fn set_session(&mut self, session: Session) -> Result<(), String> {
        self.session = session;
        if self.device.is_some() { session::apply(session) } else { Ok(()) }
    }

    pub fn session(&self) -> Session {
        self.session
    }

    fn send(&mut self, command: Command) -> Result<(), String> {
        self.commands.push(command).map_err(|_| format!("more than {COMMANDS} sound commands in one frame"))
    }

    fn slot(&self, handle: Handle) -> Option<usize> {
        let slot = (handle & 0xFF) as usize;
        let generation = (handle >> 8) as u32;
        let s = self.slots.get(slot)?;
        (s.busy && s.generation == generation).then_some(slot)
    }

    fn handle(&self, slot: usize) -> Handle {
        (u64::from(self.slots[slot].generation) << 8) | slot as u64
    }

    /// Plays `clip`. `Ok(None)` if every voice is busy with one at least as
    /// important; a stolen voice is in the next [`Audio::stopped`].
    pub fn play(&mut self, clip: &Arc<Clip>, play: Play) -> Result<Option<Handle>, String> {
        check_settings(play.settings)?;
        check_fade(play.fade_in)?;
        if play.priority > LOWEST_PRIORITY {
            return Err(format!("priority {} isn't between 0 and {LOWEST_PRIORITY}", play.priority));
        }
        if !(play.from.is_finite() && play.from >= 0.0) {
            return Err(format!("a sound can't start at {} s", play.from));
        }
        play.spatial.check()?;
        check_position(play.position)?;
        if self.commands.slots() == 0 {
            return Err(format!("more than {COMMANDS} sound commands in one frame"));
        }
        let slot = match self.slots.iter().position(|s| !s.busy) {
            Some(free) => free,
            None => {
                // The least important at or below this one's importance,
                // the oldest of equals, and one already stopping before all.
                let victim = (0..MAX_VOICES)
                    .filter(|&i| self.slots[i].stopped || self.slots[i].priority >= play.priority)
                    .max_by_key(|&i| (self.slots[i].stopped, self.slots[i].priority, u64::MAX - self.slots[i].order));
                let Some(victim) = victim else {
                    self.stats.refused += 1;
                    return Ok(None);
                };
                if !self.slots[victim].stopped {
                    self.stats.stolen += 1;
                    let (handle, owner) = (self.handle(victim), self.slots[victim].owner);
                    self.stopped.push(Stopped { handle, owner, stolen: true });
                }
                victim
            }
        };
        let generation = self.slots[slot].generation.wrapping_add(1).max(1);
        self.order += 1;
        let mix = spatial::mix(&play.spatial, play.position, &self.listener);
        self.slots[slot] = Slot {
            generation,
            busy: true,
            stopped: false,
            priority: play.priority,
            owner: play.owner,
            order: self.order,
            rate: clip.rate(),
            spatial: play.spatial,
            position: play.position,
            mix,
        };
        let from = play.from * f64::from(clip.rate());
        let stream = clip.streamed().then(|| {
            let (tap, feed) = stream::open(clip, from as u64, play.settings.looping);
            self.streamer.add(feed);
            tap
        });
        self.send(Command::Start {
            slot: slot as u8,
            generation,
            clip: clip.clone(),
            stream,
            settings: play.settings,
            pausable: play.pausable,
            from,
            mix,
            fade_in: play.fade_in,
        })?;
        self.stats.started += 1;
        Ok(Some(self.handle(slot)))
    }

    /// Stops a voice (with a short fade). A voice that already ended is
    /// left alone.
    pub fn stop(&mut self, handle: Handle) -> Result<(), String> {
        let Some(slot) = self.slot(handle) else { return Ok(()) };
        if self.slots[slot].stopped {
            return Ok(());
        }
        self.slots[slot].stopped = true;
        let generation = self.slots[slot].generation;
        self.send(Command::Stop { slot: slot as u8, generation })
    }

    /// Stops every voice of `owner`.
    pub fn stop_owner(&mut self, owner: u64) -> Result<(), String> {
        for slot in 0..MAX_VOICES {
            let s = self.slots[slot];
            if s.busy && !s.stopped && s.owner == owner {
                self.stop(self.handle(slot))?;
            }
        }
        Ok(())
    }

    /// Fades a voice to `to` (0 to 1, times its volume) over `seconds`,
    /// then, when `stop`, stops it: it isn't [`Audio::playing`] from now.
    pub fn fade(&mut self, handle: Handle, to: f32, seconds: f32, stop: bool) -> Result<(), String> {
        check_unit(to, "a fade's volume")?;
        check_fade(seconds)?;
        let Some(slot) = self.slot(handle) else { return Ok(()) };
        if self.slots[slot].stopped {
            return Ok(());
        }
        let generation = self.slots[slot].generation;
        self.send(Command::Fade { slot: slot as u8, generation, to, seconds, stop })?;
        if stop {
            self.slots[slot].stopped = true;
        }
        Ok(())
    }

    /// A group's volume (0 to 1), faded to over `seconds`, or over the next
    /// callback for 0: every voice in it follows. Groups are 1 to
    /// [`MAX_GROUPS`] - 1, each at 1 until set.
    pub fn set_group(&mut self, group: u8, volume: f32, seconds: f32) -> Result<(), String> {
        if group == 0 || usize::from(group) >= MAX_GROUPS {
            return Err(format!("group {group} isn't between 1 and {}", MAX_GROUPS - 1));
        }
        check_unit(volume, "a group's volume")?;
        check_fade(seconds)?;
        self.send(Command::Group { group, to: volume, seconds })
    }

    /// Pauses a voice where it is, or plays it on.
    pub fn pause(&mut self, handle: Handle, paused: bool) -> Result<(), String> {
        let Some(slot) = self.slot(handle) else { return Ok(()) };
        let generation = self.slots[slot].generation;
        self.send(Command::Pause { slot: slot as u8, generation, paused })
    }

    /// Changes a playing voice's volume, pitch or looping.
    pub fn set(&mut self, handle: Handle, settings: Settings) -> Result<(), String> {
        check_settings(settings)?;
        let Some(slot) = self.slot(handle) else { return Ok(()) };
        let generation = self.slots[slot].generation;
        self.send(Command::Set { slot: slot as u8, generation, settings })
    }

    /// Changes a playing voice's 2D or 3D settings: heard at once.
    pub fn set_spatial(&mut self, handle: Handle, spatial: Spatial) -> Result<(), String> {
        spatial.check()?;
        let Some(slot) = self.slot(handle) else { return Ok(()) };
        self.slots[slot].spatial = spatial;
        self.remix(slot)
    }

    /// Moves a 3D voice to `position`: heard from the next
    /// [`Audio::update`], when every move of the frame is sent at once.
    pub fn place(&mut self, handle: Handle, position: [f32; 3]) -> Result<(), String> {
        check_position(position)?;
        let Some(slot) = self.slot(handle) else { return Ok(()) };
        if self.slots[slot].position != position {
            self.slots[slot].position = position;
            self.moved |= self.slots[slot].spatial.blend > 0.0;
        }
        Ok(())
    }

    /// Who hears, from the next [`Audio::update`]; at the origin looking
    /// along -Z until set.
    pub fn set_listener(&mut self, listener: Listener) -> Result<(), String> {
        listener.check()?;
        if self.listener != listener {
            self.listener = listener;
            self.moved = true;
        }
        Ok(())
    }

    pub fn listener(&self) -> Listener {
        self.listener
    }

    /// How a playing voice is heard now: the mix last sent for it.
    pub fn mix(&self, handle: Handle) -> Option<Mix> {
        self.slot(handle).map(|slot| self.slots[slot].mix)
    }

    /// Works out a voice's mix again, and sends it if it changed.
    fn remix(&mut self, slot: usize) -> Result<(), String> {
        let s = self.slots[slot];
        let mix = spatial::mix(&s.spatial, s.position, &self.listener);
        if mix == s.mix {
            return Ok(());
        }
        self.slots[slot].mix = mix;
        self.send(Command::Mix { slot: slot as u8, generation: s.generation, mix })
    }

    /// Whether the voice plays: started, not stopped, not ended.
    pub fn playing(&self, handle: Handle) -> bool {
        self.slot(handle).is_some_and(|slot| !self.slots[slot].stopped)
    }

    /// Where the voice is in its clip, in seconds, as of the last callback
    /// (0 before its first); `None` once it isn't [`Audio::playing`].
    pub fn time(&self, handle: Handle) -> Option<f64> {
        let slot = self.slot(handle)?;
        let s = &self.slots[slot];
        if s.stopped {
            return None;
        }
        let frame = self.shared.voices[slot].read(s.generation).unwrap_or(0.0);
        Some(frame / f64::from(s.rate))
    }

    /// Whether the game is paused: pausable voices fall silent and hold.
    pub fn set_game_paused(&mut self, paused: bool) -> Result<(), String> {
        self.send(Command::GamePaused(paused))
    }

    /// Every voice's gain, 0 to 1.
    pub fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        check_unit(volume, "volume")?;
        self.send(Command::Volume(volume))
    }

    /// Once a frame, with the frame's real time in seconds: takes back the
    /// voices that ended (see [`Audio::stopped`]), renders the time
    /// without a device, and opens a lost device again (at most once a
    /// second).
    pub fn update(&mut self, seconds: f64) {
        self.stopped.clear();
        if self.moved {
            self.moved = false;
            for slot in 0..MAX_VOICES {
                let s = &self.slots[slot];
                if s.busy && !s.stopped && s.spatial.blend > 0.0 && self.remix(slot).is_err() {
                    // The ring is full this frame: tried again next frame.
                    self.moved = true;
                }
            }
        }
        if let Some(device) = &self.device
            && device.lost.load(Ordering::Relaxed)
        {
            self.drop_device();
        }
        if self.device.is_none()
            && self.wants_device
            && self.last_try.is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
            && self.try_device().is_ok()
        {
            self.stats.reopened += 1;
        }
        if self.device.is_some() && self.last_call_check.is_none_or(|at| at.elapsed() >= CALL_CHECK) {
            self.last_call_check = Some(Instant::now());
            let in_call = session::in_call();
            if in_call != self.in_call {
                self.in_call = in_call;
                if let Some(device) = &self.device {
                    // A device that can't pause or play again is lost, and
                    // opened again.
                    let _ = device.suspend(self.suspended || in_call);
                }
            }
        }
        if self.device.is_none() && !self.suspended && seconds.is_finite() && seconds > 0.0 {
            self.owed += seconds.min(1.0) * f64::from(QUIET_RATE);
            let frames = self.owed.floor() as usize;
            self.owed -= frames as f64;
            self.render_quietly(frames);
        }
        self.take_returns();
    }

    fn render_quietly(&mut self, mut frames: usize) {
        let Ok(mut mixer) = self.mixer.lock() else { return };
        let channels = mixer.channels();
        let chunk = self.scratch.len() / channels;
        while frames > 0 {
            let n = frames.min(chunk);
            self.streamer.pump();
            mixer.render(&mut self.scratch[..n * channels]);
            frames -= n;
        }
    }

    fn take_returns(&mut self) {
        while let Ok(ended) = self.returns.pop() {
            let slot = usize::from(ended.slot);
            let Some(s) = self.slots.get_mut(slot) else { continue };
            if s.busy && s.generation == ended.generation {
                s.busy = false;
                if ended.finished && !s.stopped {
                    let handle = (u64::from(s.generation) << 8) | slot as u64;
                    self.stopped.push(Stopped { handle, owner: s.owner, stolen: false });
                }
            }
            // The clip and a stream's ring are dropped here, off the audio
            // thread; the ring's decoder sees it's gone and stops.
            drop(ended.clip);
            drop(ended.stream);
        }
    }

    /// The voices that stopped on their own in the last [`Audio::update`]:
    /// those that reached their end, and those stolen since.
    pub fn stopped(&self) -> &[Stopped] {
        &self.stopped
    }

    /// Renders `out` (interleaved stereo at [`QUIET_RATE`]) as the device
    /// would have, for tests. Fails while a device plays.
    pub fn render(&mut self, out: &mut [f32]) -> Result<(), String> {
        if self.device.is_some() {
            return Err("a device plays the sound".into());
        }
        {
            let mut mixer = self.mixer.lock().map_err(|_| "the mixer panicked")?;
            if self.streamer.busy() {
                // In pieces, decoding streams between, as with a device.
                for piece in out.chunks_mut(CHUNK * 2) {
                    self.streamer.pump();
                    mixer.render(piece);
                }
            } else {
                mixer.render(out);
            }
        }
        self.take_returns();
        Ok(())
    }

    /// Decodes what every streamed voice needs next, as [`Audio::render`]
    /// does first: for a test of the mixer alone, with
    /// [`Audio::render_mixer`].
    #[doc(hidden)]
    pub fn decode_streams(&mut self) {
        self.streamer.pump();
    }

    /// Renders as [`Audio::render`], but without decoding streams first
    /// or taking back the voices that ended: the mixer alone, as the audio
    /// thread runs it.
    #[doc(hidden)]
    pub fn render_mixer(&mut self, out: &mut [f32]) -> Result<(), String> {
        if self.device.is_some() {
            return Err("a device plays the sound".into());
        }
        self.mixer.lock().map_err(|_| "the mixer panicked")?.render(out);
        Ok(())
    }

    pub fn stats(&self) -> Stats {
        let (rate, channels) = match &self.device {
            Some(device) => (device.rate, device.channels as u32),
            None => (QUIET_RATE, 2),
        };
        let underruns = self.device.as_ref().map_or(0, |d| d.underruns.load(Ordering::Relaxed));
        Stats {
            device: self.device.is_some(),
            rate,
            channels,
            voices: self.shared.playing.load(Ordering::Relaxed),
            callbacks: self.shared.callbacks.load(Ordering::Relaxed),
            underruns: self.stats.underruns + underruns,
            starved: self.shared.starved.load(Ordering::Relaxed),
            ..self.stats
        }
    }
}

fn check_position(position: [f32; 3]) -> Result<(), String> {
    if position.iter().all(|v| v.is_finite()) { Ok(()) } else { Err(format!("a sound can't be at {position:?}")) }
}

fn check_unit(value: f32, what: &str) -> Result<(), String> {
    if (0.0..=1.0).contains(&value) { Ok(()) } else { Err(format!("{what} {value} isn't between 0 and 1")) }
}

fn check_fade(seconds: f32) -> Result<(), String> {
    if (0.0..=MAX_FADE).contains(&seconds) {
        Ok(())
    } else {
        Err(format!("a fade of {seconds} s isn't between 0 and {MAX_FADE}"))
    }
}

fn check_settings(settings: Settings) -> Result<(), String> {
    check_unit(settings.volume, "volume")?;
    if usize::from(settings.group) >= MAX_GROUPS {
        return Err(format!("group {} isn't between 0 and {}", settings.group, MAX_GROUPS - 1));
    }
    if !(MIN_PITCH..=MAX_PITCH).contains(&settings.pitch) {
        return Err(format!("pitch {} isn't between {MIN_PITCH} and {MAX_PITCH}", settings.pitch));
    }
    Ok(())
}
