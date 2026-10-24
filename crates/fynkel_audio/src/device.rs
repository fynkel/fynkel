//! The output device: cpal's default, in 32-bit floats at its own rate,
//! with the mixer in its callback. The callback only ever tries the
//! mixer's lock: the game's side takes it only while no device plays (to
//! render without one) or while a lost device is replaced, so it's never
//! held when the callback comes.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, SampleFormat, StreamConfig};

use crate::mixer::Mixer;
use crate::session;

pub(crate) struct Device {
    stream: cpal::Stream,
    pub(crate) rate: u32,
    pub(crate) channels: usize,
    /// The device went away (unplugged, or the system took it): it must be
    /// opened again.
    pub(crate) lost: Arc<AtomicBool>,
    pub(crate) underruns: Arc<AtomicU64>,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device").field("rate", &self.rate).field("channels", &self.channels).finish()
    }
}

impl Device {
    /// Opens the default output and starts it, playing `mixer`. A panic in
    /// the system's sound (cpal) is an error here, as no device: the mixer
    /// isn't touched by it, so sound goes on without one.
    pub(crate) fn open(mixer: &Arc<Mutex<Mixer>>) -> Result<Self, String> {
        #[cfg(target_os = "android")]
        crate::android::prepare()?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Self::open_now(mixer))).unwrap_or_else(|payload| {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic".to_owned());
            Err(format!("the sound output failed: {message}"))
        })
    }

    fn open_now(mixer: &Arc<Mutex<Mixer>>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("there's no sound output")?;
        let default = device.default_output_config().map_err(|e| format!("the sound output: {e}"))?;
        let config: StreamConfig = if default.sample_format() == SampleFormat::F32 {
            default.config()
        } else {
            // The device's own rate and channels, in floats if it has them.
            let rate = default.sample_rate();
            device
                .supported_output_configs()
                .map_err(|e| format!("the sound output: {e}"))?
                .find(|c| {
                    c.sample_format() == SampleFormat::F32 && c.min_sample_rate() <= rate && rate <= c.max_sample_rate()
                })
                .map(|c| c.with_sample_rate(rate).config())
                .ok_or("the sound output doesn't take 32-bit floats")?
        };
        let rate = config.sample_rate;
        let channels = usize::from(config.channels);
        mixer.lock().map_err(|_| "the mixer panicked")?.set_output(rate, channels);

        let lost = Arc::new(AtomicBool::new(false));
        let underruns = Arc::new(AtomicU64::new(0));
        let (lost_in, underruns_in, playing) = (lost.clone(), underruns.clone(), mixer.clone());
        let opened = Arc::new(Mutex::new(None));
        let opened_in = opened.clone();
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| match playing.try_lock() {
                    Ok(mut mixer) => mixer.render(out),
                    Err(_) => out.fill(0.0),
                },
                move |error: cpal::Error| match error.kind() {
                    ErrorKind::Xrun => {
                        underruns_in.fetch_add(1, Ordering::Relaxed);
                    }
                    // A route change that kept the output (iOS): the stream
                    // plays on. Should it not, the callbacks stop, and
                    // `Audio::update` finds it lost.
                    ErrorKind::StreamInvalidated
                        if opened_in
                            .lock()
                            .is_ok_and(|o| o.as_ref().is_some_and(|o| Some(o) == session::output().as_ref())) => {}
                    ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated => {
                        lost_in.store(true, Ordering::Relaxed)
                    }
                    _ => {}
                },
                None,
            )
            .map_err(|e| format!("the sound output can't be opened: {e}"))?;
        stream.play().map_err(|e| format!("the sound output can't start: {e}"))?;
        if let Ok(mut o) = opened.lock() {
            *o = session::output();
        }
        Ok(Self { stream, rate, channels, lost, underruns })
    }

    /// Stops the output while the app is away, or starts it again.
    pub(crate) fn suspend(&self, suspended: bool) -> Result<(), String> {
        if suspended { self.stream.pause() } else { self.stream.play() }.map_err(|e| format!("the sound output: {e}"))
    }
}
