//! Where the mixer runs: inside the engine (`Null`, rendered on demand by
//! a test or the headless host) or inside an output device's callback
//! (`Device`, `cpal` behind the `device` feature).

use crate::mixer::Mixer;

/// The output format the mixer renders for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub sample_rate: u32,
    pub channels: usize,
    /// A real device (else the null backend).
    pub device: bool,
}

pub(crate) enum Backend {
    Null(Box<Mixer>),
    /// Uninhabited without the `device` feature, so the engine's code is
    /// the same either way.
    #[cfg_attr(not(feature = "device"), allow(dead_code))]
    Device(cpal::DeviceStream),
}

impl Backend {
    pub(crate) fn info(&self) -> DeviceInfo {
        match self {
            Backend::Null(m) => DeviceInfo {
                sample_rate: m.sample_rate(),
                channels: m.channels(),
                device: false,
            },
            Backend::Device(d) => d.info(),
        }
    }

    pub(crate) fn underruns(&self) -> u64 {
        match self {
            Backend::Null(_) => 0,
            Backend::Device(d) => d.underruns(),
        }
    }

    pub(crate) fn set_suspended(&mut self, suspended: bool) {
        match self {
            Backend::Null(_) => {
                let _ = suspended;
            }
            Backend::Device(d) => d.set_suspended(suspended),
        }
    }
}

#[cfg(not(feature = "device"))]
pub(crate) mod cpal {
    use super::DeviceInfo;

    /// No device without the feature.
    pub(crate) enum DeviceStream {}

    impl DeviceStream {
        pub(crate) fn info(&self) -> DeviceInfo {
            match *self {}
        }

        pub(crate) fn underruns(&self) -> u64 {
            match *self {}
        }

        pub(crate) fn set_suspended(&mut self, _suspended: bool) {
            match *self {}
        }
    }
}

#[cfg(feature = "device")]
pub(crate) mod cpal {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use ::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use super::DeviceInfo;
    use crate::mixer::{Command, Mixer, MixerStats, Return};

    pub(crate) struct DeviceStream {
        info: DeviceInfo,
        stream: ::cpal::Stream,
        underruns: Arc<AtomicU64>,
    }

    impl DeviceStream {
        pub(crate) fn info(&self) -> DeviceInfo {
            self.info
        }

        pub(crate) fn underruns(&self) -> u64 {
            self.underruns.load(Ordering::Relaxed)
        }

        pub(crate) fn set_suspended(&mut self, suspended: bool) {
            // A failure here is not fatal: the stream keeps its state and
            // the next call retries.
            let _ = if suspended {
                self.stream.pause()
            } else {
                self.stream.play()
            };
        }
    }

    /// Opens the default output device at its default configuration
    /// (`f32` samples; the mixer renders at the device's rate and channel
    /// count) and starts the stream.
    pub(crate) fn open(
        commands: rtrb::Consumer<Command>,
        returns: rtrb::Producer<Return>,
        stats: Arc<MixerStats>,
    ) -> Result<DeviceStream, String> {
        let host = ::cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no output device".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|e| format!("no output configuration: {e}"))?;
        let config = supported.config();
        let sample_rate = config.sample_rate;
        let channels = config.channels as usize;
        let mut mixer = Mixer::new(sample_rate, channels, commands, returns, stats);
        let underruns = Arc::new(AtomicU64::new(0));
        let counter = underruns.clone();
        let stream = device
            .build_output_stream(
                config,
                move |out: &mut [f32], _: &::cpal::OutputCallbackInfo| mixer.render(out),
                move |err: ::cpal::Error| {
                    if err.kind() == ::cpal::ErrorKind::Xrun {
                        counter.fetch_add(1, Ordering::Relaxed);
                    }
                },
                None,
            )
            .map_err(|e| format!("cannot open the output stream: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("cannot start the output stream: {e}"))?;
        Ok(DeviceStream {
            info: DeviceInfo {
                sample_rate,
                channels,
                device: true,
            },
            stream,
            underruns,
        })
    }
}
