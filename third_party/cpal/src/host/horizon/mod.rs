//! Nintendo Switch homebrew host (libnx's audout), added by Horizon's Switch port
//! (docs/switch.md). One output device, 16-bit stereo at the service's rate (48 kHz); a
//! stream is a thread that fills one buffer at a time from the callback and hands it to
//! the service, which plays a short ring of them. No input.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError,
    DevicesError, InputCallbackInfo, OutputCallbackInfo, OutputStreamTimestamp, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, StreamInstant,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

/// Frames per buffer: 1024 at 48 kHz is 21 ms; four in flight keep about 85 ms queued.
const FRAMES: u32 = 1024;
const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

#[derive(Default)]
pub struct Devices {
    given: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

pub struct Stream {
    playing: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices::default())
    }
}

fn config_range() -> SupportedStreamConfigRange {
    SupportedStreamConfigRange::new(
        CHANNELS,
        SampleRate(RATE),
        SampleRate(RATE),
        SupportedBufferSize::Range { min: FRAMES, max: FRAMES },
        SampleFormat::I16,
    )
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("Nintendo Switch".to_owned())
    }

    fn supported_input_configs(&self) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        Ok(Vec::new().into_iter())
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(vec![config_range()].into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Err(DefaultStreamConfigError::StreamTypeNotSupported)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Ok(SupportedStreamConfig::new(
            CHANNELS,
            SampleRate(RATE),
            SupportedBufferSize::Range { min: FRAMES, max: FRAMES },
            SampleFormat::I16,
        ))
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Err(BuildStreamError::StreamConfigNotSupported)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if sample_format != SampleFormat::I16 || config.channels != CHANNELS || config.sample_rate.0 != RATE {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }
        let Some((rate, _)) = horizon_switch::audio::open(FRAMES) else {
            return Err(BuildStreamError::BackendSpecific {
                err: BackendSpecificError { description: "audout did not open".into() },
            });
        };
        let frames = horizon_switch::audio::frames();
        let playing = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_playing, thread_stop) = (playing.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("Audio output".into())
            .stack_size(256 << 10)
            .spawn(move || {
                let mut buffer = vec![0i16; frames as usize * CHANNELS as usize];
                let started = Instant::now();
                let per_buffer = Duration::from_secs_f64(f64::from(frames) / f64::from(rate.max(1)));
                let mut failures = 0u32;
                while !thread_stop.load(Ordering::Relaxed) {
                    if thread_playing.load(Ordering::Relaxed) {
                        let elapsed = started.elapsed();
                        let callback = StreamInstant::new(elapsed.as_secs() as i64, elapsed.subsec_nanos());
                        let playback = callback.add(per_buffer * 3).unwrap_or(callback);
                        let info = OutputCallbackInfo::new(OutputStreamTimestamp { callback, playback });
                        let len = buffer.len();
                        let mut data = unsafe { Data::from_parts(buffer.as_mut_ptr().cast(), len, SampleFormat::I16) };
                        data_callback(&mut data, &info);
                    } else {
                        buffer.fill(0);
                    }
                    if horizon_switch::audio::write(&buffer) {
                        failures = 0;
                    } else {
                        failures += 1;
                        if failures == 10 {
                            error_callback(StreamError::DeviceNotAvailable);
                        }
                        std::thread::sleep(per_buffer);
                    }
                }
            })
            .map_err(|e| BuildStreamError::BackendSpecific {
                err: BackendSpecificError { description: e.to_string() },
            })?;
        Ok(Stream { playing, stop, thread: Some(thread) })
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Device> {
        None
    }

    fn default_output_device(&self) -> Option<Device> {
        Some(Device)
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.playing.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.playing.store(false, Ordering::Relaxed);
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        if self.given {
            return None;
        }
        self.given = true;
        Some(Device)
    }
}
