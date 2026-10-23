//! Streamed clips (docs/stage-11.md §3.2): each voice of one has a ring of
//! samples, a second long, that the mixer reads ([`Tap`]) and a decoder
//! keeps full ([`Feed`]). With a device, the decoding is on a thread of
//! its own ([`Streamer`]); without one, the game's side does it between
//! renders, so a test hears the same every time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::clip::{Clip, Reader};

/// How often the decoder thread looks at its streams, at the least: well
/// within the second each has decoded ahead.
const WAKE: Duration = Duration::from_millis(20);

/// What a voice's stream and its decoder share.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// Whether it starts again at the end: the voice's setting.
    pub looping: AtomicBool,
    /// Everything there is has gone into the ring: once it's empty, the
    /// voice is done. Written after the last samples.
    pub ended: AtomicBool,
}

/// The mixer's end of a stream.
#[derive(Debug)]
pub(crate) struct Tap {
    pub samples: rtrb::Consumer<f32>,
    pub state: Arc<State>,
}

/// The decoder's end of a stream.
#[derive(Debug)]
pub(crate) struct Feed {
    reader: Reader,
    samples: rtrb::Producer<f32>,
    state: Arc<State>,
}

impl Feed {
    /// Decodes until its ring is full; false once its voice is gone.
    fn fill(&mut self) -> bool {
        if self.samples.is_abandoned() {
            return false;
        }
        if !self.reader.ended {
            self.reader.fill(&mut self.samples, self.state.looping.load(Ordering::Relaxed));
            if self.reader.ended {
                self.state.ended.store(true, Ordering::Release);
            }
        }
        true
    }
}

/// A stream of `clip` from frame `from`: the mixer's end and the
/// decoder's.
pub(crate) fn open(clip: &Arc<Clip>, from: u64, looping: bool) -> (Tap, Feed) {
    let channels = usize::from(clip.channels());
    let frames = (clip.rate() as usize).max(4_096);
    let (samples_in, samples_out) = rtrb::RingBuffer::new(frames * channels);
    let state = Arc::new(State { looping: AtomicBool::new(looping), ended: AtomicBool::new(false) });
    let feed = Feed { reader: Reader::new(clip.clone(), from), samples: samples_in, state: state.clone() };
    (Tap { samples: samples_out, state }, feed)
}

#[derive(Debug)]
struct Worker {
    feeds: mpsc::Sender<Feed>,
    quit: Arc<AtomicBool>,
    thread: thread::JoinHandle<Vec<Feed>>,
}

/// Every stream's decoder: on its own thread while a device plays, or on
/// the game's.
#[derive(Debug, Default)]
pub(crate) struct Streamer {
    /// Without a thread, the streams the game's side fills.
    here: Vec<Feed>,
    worker: Option<Worker>,
}

impl Streamer {
    /// Starts decoding `feed`.
    pub(crate) fn add(&mut self, feed: Feed) {
        match &self.worker {
            Some(worker) => {
                // Sent, and the thread woken to fill it at once. A thread
                // that's gone (it panicked) leaves the voice waiting.
                if worker.feeds.send(feed).is_ok() {
                    worker.thread.thread().unpark();
                }
            }
            None => self.here.push(feed),
        }
    }

    /// Without a thread, fills every stream; drops those whose voices are
    /// gone.
    pub(crate) fn pump(&mut self) {
        if self.worker.is_none() {
            self.here.retain_mut(Feed::fill);
        }
    }

    /// Whether any stream is being decoded.
    pub(crate) fn busy(&self) -> bool {
        self.worker.is_some() || !self.here.is_empty()
    }

    /// Decodes on a thread of its own from now on, until [`Streamer::stop`].
    pub(crate) fn start(&mut self) -> Result<(), String> {
        if self.worker.is_some() {
            return Ok(());
        }
        let (send, take) = mpsc::channel::<Feed>();
        let quit = Arc::new(AtomicBool::new(false));
        let quitting = quit.clone();
        let mut feeds = std::mem::take(&mut self.here);
        let thread = thread::Builder::new()
            .name("fynkel sound streams".into())
            .spawn(move || {
                loop {
                    feeds.extend(take.try_iter());
                    feeds.retain_mut(Feed::fill);
                    if quitting.load(Ordering::Relaxed) {
                        feeds.extend(take.try_iter());
                        return feeds;
                    }
                    thread::park_timeout(WAKE);
                }
            })
            .map_err(|e| format!("the sound's stream thread can't start: {e}"))?;
        self.worker = Some(Worker { feeds: send, quit, thread });
        Ok(())
    }

    /// Decodes on the game's side again, with the streams the thread had.
    pub(crate) fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.quit.store(true, Ordering::Relaxed);
            worker.thread.thread().unpark();
            if let Ok(feeds) = worker.thread.join() {
                self.here = feeds;
            }
        }
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop();
    }
}
