//! ALSA capture/playback adapter implementing [`MediaIo`] for the engine.
//!
//! # Threading model
//!
//! * **Device callbacks** (cpal/ALSA threads) only convert samples and move
//!   them through preallocated lock-free SPSC rings at the device rate. They
//!   never lock, block or allocate. The capture callback downmixes to mono;
//!   the playback callback upmixes mono to every device channel and emits
//!   silence on underrun.
//! * **Engine thread** calls [`MediaIo::capture`] / [`MediaIo::play`] every
//!   20 ms. All resampling (device rate ↔ 8 kHz, see [`Resampler`]) happens
//!   there, so callbacks stay trivial.
//! * **UI thread** reads counters and the input level through a cloned
//!   [`AudioMonitor`] (atomics only).
//!
//! # Limits
//!
//! * Device rings hold 200 ms. A full capture ring drops the newest samples;
//!   the 8 kHz capture queue keeps at most 60 ms and drops the oldest samples,
//!   bounding capture latency. Playback drops whole incoming 20 ms frames once
//!   120 ms are queued, and re-primes with 40 ms after an underrun.
//! * There is no clock-drift compensation between the device clock and the
//!   engine timer; drift shows up as occasional counted drops or underruns.
//! * Only the default cpal host (ALSA, including its PipeWire/Pulse plugins) is
//!   used. Formats: `i16`, `i32`, `u16` and `f32`; any rate and channel count.
//! * Opening never panics: failures return [`AudioError`] so the caller can
//!   fall back to [`rust_microsip_core::NullMedia`] and report
//!   [`rust_microsip_core::Notice::AudioUnavailable`]. Errors after start are
//!   latched in [`AudioStatus::stream_failed`].

mod resample;
mod ring;

pub use resample::Resampler;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use ring::{Consumer, Producer, ring};
use rust_microsip_core::{FRAME_SAMPLES, MediaIo};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::time::Duration;

/// Engine-side sample rate.
const ENGINE_RATE: u32 = 8_000;
/// Device ring capacity, in milliseconds.
const RING_MS: u32 = 200;
/// Maximum 8 kHz capture backlog before the oldest samples are dropped.
const CAPTURE_QUEUE_MAX: usize = 480;
/// Playback backlog above which new frames are dropped, in milliseconds.
const PLAYBACK_MAX_MS: u32 = 120;
/// Playback level required before output resumes after start or underrun.
const PLAYBACK_PRIME_MS: u32 = 40;
/// Maximum time to wait for the backend while building a stream.
const OPEN_TIMEOUT: Duration = Duration::from_secs(2);

/// Why audio could not be opened. The app should fall back to `NullMedia`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    /// No input (capture) device exists or none matched the selection.
    #[error("no matching audio input device")]
    NoInputDevice,
    /// No output (playback) device exists or none matched the selection.
    #[error("no matching audio output device")]
    NoOutputDevice,
    /// The device's preferred sample format is not supported by this adapter.
    #[error("unsupported audio sample format {0}")]
    UnsupportedFormat(String),
    /// The audio backend reported an error while enumerating or opening.
    #[error("audio backend error: {0}")]
    Backend(String),
}

impl From<cpal::Error> for AudioError {
    fn from(error: cpal::Error) -> Self {
        Self::Backend(error.to_string())
    }
}

/// One audio endpoint as shown in the settings UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable backend identifier (for example `alsa:default`); persist this.
    /// Empty when the backend cannot provide one.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Whether this is the host's default device for its direction.
    pub is_default: bool,
}

/// Snapshot of the host's capture and playback devices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioDevices {
    /// Devices that support capture.
    pub inputs: Vec<DeviceInfo>,
    /// Devices that support playback.
    pub outputs: Vec<DeviceInfo>,
}

impl AudioDevices {
    /// Enumerate devices on the default host. This may probe hardware and take
    /// a moment; call it from a worker or settings dialog, not per frame.
    ///
    /// # Errors
    /// Returns [`AudioError::Backend`] when the host cannot list devices.
    pub fn enumerate() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let describe = |devices: &mut dyn Iterator<Item = Device>, default: Option<Device>| {
            devices
                .map(|d| DeviceInfo {
                    id: d.id().map(|id| id.to_string()).unwrap_or_default(),
                    name: d.to_string(),
                    is_default: default.as_ref() == Some(&d),
                })
                .collect()
        };
        Ok(Self {
            inputs: routed_only(describe(
                &mut host.input_devices()?,
                host.default_input_device(),
            )),
            outputs: routed_only(describe(
                &mut host.output_devices()?,
                host.default_output_device(),
            )),
        })
    }
}

/// On a PipeWire or PulseAudio desktop the sound server owns the hardware:
/// raw ALSA PCMs (`hw`, `plughw`, `usbstream`, `surround`...) are busy or
/// unusable, so only the routed entries are listed and the device is chosen
/// in the desktop's sound settings. Plain ALSA systems keep the full list.
fn routed_only(devices: Vec<DeviceInfo>) -> Vec<DeviceInfo> {
    const ROUTED: [&str; 3] = ["alsa:default", "alsa:pulse", "alsa:pipewire"];
    let server = devices
        .iter()
        .any(|d| d.id == "alsa:pulse" || d.id == "alsa:pipewire");
    if !server {
        return devices;
    }
    devices
        .into_iter()
        .filter(|d| ROUTED.contains(&d.id.as_str()))
        .collect()
}

/// Point-in-time adapter health and counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioStatus {
    /// A stream reported an error after start (latched until reopen).
    pub stream_failed: bool,
    /// Captured samples dropped (full ring or excess backlog).
    pub capture_dropped: u64,
    /// Playback samples dropped (full ring) plus 160 per dropped frame.
    pub playback_dropped: u64,
    /// Output callbacks that ran out of samples after playback had started.
    pub playback_underruns: u64,
    /// Peak absolute value of the last captured 8 kHz frame (0-32767).
    pub input_peak: u16,
}

#[derive(Default)]
struct Stats {
    stream_failed: AtomicBool,
    capture_dropped: AtomicU64,
    playback_dropped: AtomicU64,
    playback_underruns: AtomicU64,
    input_peak: AtomicU16,
}

/// Cheap, cloneable read-only view of a [`DeviceMedia`]'s status for the UI.
#[derive(Clone)]
pub struct AudioMonitor(Arc<Stats>);

impl AudioMonitor {
    /// Read all counters (relaxed atomics; values may be slightly stale).
    pub fn status(&self) -> AudioStatus {
        let s = &self.0;
        AudioStatus {
            stream_failed: s.stream_failed.load(Ordering::Relaxed),
            capture_dropped: s.capture_dropped.load(Ordering::Relaxed),
            playback_dropped: s.playback_dropped.load(Ordering::Relaxed),
            playback_underruns: s.playback_underruns.load(Ordering::Relaxed),
            input_peak: s.input_peak.load(Ordering::Relaxed),
        }
    }
}

/// Convert a float sample in `[-1, 1]` to `i16`, saturating; NaN becomes 0.
pub fn f32_to_i16(x: f32) -> i16 {
    // `as` saturates out-of-range values and maps NaN to 0.
    (x * 32_768.0).round() as i16
}

/// Convert an `i16` sample to a float in `[-1, 1)`.
pub fn i16_to_f32(x: i16) -> f32 {
    f32::from(x) / 32_768.0
}

/// Capture callback body: downmix interleaved frames to mono and queue them.
fn capture_callback<T: Sample>(data: &[T], channels: usize, tx: &mut Producer, stats: &Stats)
where
    f32: FromSample<T>,
{
    let mut dropped = 0;
    for frame in data.chunks_exact(channels.max(1)) {
        let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
        if !tx.push(f32_to_i16(sum / frame.len() as f32)) {
            dropped += 1;
        }
    }
    if dropped > 0 {
        stats.capture_dropped.fetch_add(dropped, Ordering::Relaxed);
    }
}

/// Playback callback state: whether output is primed after start/underrun.
struct PlaybackState {
    primed: bool,
    prime_level: usize,
}

/// Playback callback body: upmix queued mono samples, silence when not primed.
fn playback_callback<T: Sample + FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    rx: &mut Consumer,
    state: &mut PlaybackState,
    stats: &Stats,
) {
    if !state.primed && rx.len() >= state.prime_level {
        state.primed = true;
    }
    let mut underrun = false;
    for frame in data.chunks_exact_mut(channels.max(1)) {
        let next = if state.primed { rx.pop() } else { None };
        let value = match next {
            Some(s) => i16_to_f32(s),
            None => {
                underrun |= state.primed;
                state.primed = false;
                0.0
            }
        };
        frame.fill(T::from_sample(value));
    }
    if underrun {
        stats.playback_underruns.fetch_add(1, Ordering::Relaxed);
    }
}

// ponytail: no clock-drift compensation; fixed backlog caps turn drift into counted
// drops/underruns. Add adaptive resampling ratio if long calls show audible glitches.
/// Engine-side half: resampling and queue policy, independent of cpal.
struct Bridge {
    capture_rx: Consumer,
    capture_rs: Resampler,
    capture_scratch: Vec<f32>,
    capture_queue: VecDeque<i16>,
    playback_tx: Producer,
    playback_rs: Resampler,
    playback_max: usize,
    stats: Arc<Stats>,
}

impl Bridge {
    fn new(
        capture_rx: Consumer,
        capture_rate: u32,
        playback_tx: Producer,
        playback_rate: u32,
        stats: Arc<Stats>,
    ) -> Self {
        Self {
            capture_scratch: Vec::with_capacity(capture_rx.capacity()),
            capture_rx,
            capture_rs: Resampler::new(capture_rate, ENGINE_RATE),
            capture_queue: VecDeque::with_capacity(CAPTURE_QUEUE_MAX),
            playback_tx,
            playback_rs: Resampler::new(ENGINE_RATE, playback_rate),
            playback_max: ms_samples(playback_rate, PLAYBACK_MAX_MS),
            stats,
        }
    }
}

impl MediaIo for Bridge {
    fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
        self.capture_scratch.clear();
        while self.capture_scratch.len() < self.capture_scratch.capacity() {
            match self.capture_rx.pop() {
                Some(s) => self.capture_scratch.push(i16_to_f32(s)),
                None => break,
            }
        }
        let mut trimmed = 0;
        let queue = &mut self.capture_queue;
        self.capture_rs.process(&self.capture_scratch, |y| {
            if queue.len() >= CAPTURE_QUEUE_MAX {
                queue.pop_front();
                trimmed += 1;
            }
            queue.push_back(f32_to_i16(y));
        });
        if trimmed > 0 {
            self.stats
                .capture_dropped
                .fetch_add(trimmed, Ordering::Relaxed);
        }
        if self.capture_queue.len() < FRAME_SAMPLES {
            frame.fill(0);
            return false;
        }
        for (out, s) in frame
            .iter_mut()
            .zip(self.capture_queue.drain(..FRAME_SAMPLES))
        {
            *out = s;
        }
        let peak = frame
            .iter()
            .map(|s| s.unsigned_abs().min(i16::MAX as u16))
            .max()
            .unwrap_or(0);
        self.stats.input_peak.store(peak, Ordering::Relaxed);
        true
    }

    fn play(&mut self, frame: &[i16; FRAME_SAMPLES]) {
        if self.playback_tx.len() >= self.playback_max {
            self.stats
                .playback_dropped
                .fetch_add(FRAME_SAMPLES as u64, Ordering::Relaxed);
            return;
        }
        let input = frame.map(i16_to_f32);
        let mut dropped = 0;
        let tx = &mut self.playback_tx;
        self.playback_rs.process(&input, |y| {
            if !tx.push(f32_to_i16(y)) {
                dropped += 1;
            }
        });
        if dropped > 0 {
            self.stats
                .playback_dropped
                .fetch_add(dropped, Ordering::Relaxed);
        }
    }
}

fn ms_samples(rate: u32, ms: u32) -> usize {
    (u64::from(rate) * u64::from(ms) / 1000) as usize
}

/// Live ALSA capture and playback bridged to 8 kHz mono [`MediaIo`].
///
/// Dropping it stops both streams.
pub struct DeviceMedia {
    bridge: Bridge,
    _input: Stream,
    _output: Stream,
}

impl DeviceMedia {
    /// Open and start capture and playback.
    ///
    /// `input`/`output` select a device by [`DeviceInfo::id`] or exact
    /// [`DeviceInfo::name`]; `None` uses the host default. Each stream uses the
    /// device's default configuration.
    ///
    /// # Errors
    /// Returns [`AudioError`] when a device is missing, its format is
    /// unsupported, or the backend refuses to build or start a stream.
    pub fn open(input: Option<&str>, output: Option<&str>) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let input_device = match input {
            None => host.default_input_device(),
            Some(sel) => host.input_devices()?.find(|d| matches(d, sel)),
        }
        .ok_or(AudioError::NoInputDevice)?;
        let output_device = match output {
            None => host.default_output_device(),
            Some(sel) => host.output_devices()?.find(|d| matches(d, sel)),
        }
        .ok_or(AudioError::NoOutputDevice)?;

        let stats = Arc::new(Stats::default());
        let in_cfg = input_device.default_input_config()?;
        let out_cfg = output_device.default_output_config()?;
        let (capture_tx, capture_rx) = ring(ms_samples(in_cfg.sample_rate(), RING_MS));
        let (playback_tx, playback_rx) = ring(ms_samples(out_cfg.sample_rate(), RING_MS));
        let bridge = Bridge::new(
            capture_rx,
            in_cfg.sample_rate(),
            playback_tx,
            out_cfg.sample_rate(),
            Arc::clone(&stats),
        );

        let in_config = in_cfg.config();
        let out_config = out_cfg.config();
        let input_stream = match in_cfg.sample_format() {
            SampleFormat::I16 => build_input::<i16>(&input_device, in_config, capture_tx, &stats),
            SampleFormat::I32 => build_input::<i32>(&input_device, in_config, capture_tx, &stats),
            SampleFormat::U16 => build_input::<u16>(&input_device, in_config, capture_tx, &stats),
            SampleFormat::F32 => build_input::<f32>(&input_device, in_config, capture_tx, &stats),
            other => return Err(AudioError::UnsupportedFormat(other.to_string())),
        }?;
        let state = PlaybackState {
            primed: false,
            prime_level: ms_samples(out_cfg.sample_rate(), PLAYBACK_PRIME_MS),
        };
        let output_stream = match out_cfg.sample_format() {
            SampleFormat::I16 => {
                build_output::<i16>(&output_device, out_config, playback_rx, state, &stats)
            }
            SampleFormat::I32 => {
                build_output::<i32>(&output_device, out_config, playback_rx, state, &stats)
            }
            SampleFormat::U16 => {
                build_output::<u16>(&output_device, out_config, playback_rx, state, &stats)
            }
            SampleFormat::F32 => {
                build_output::<f32>(&output_device, out_config, playback_rx, state, &stats)
            }
            other => return Err(AudioError::UnsupportedFormat(other.to_string())),
        }?;
        input_stream.play()?;
        output_stream.play()?;
        Ok(Self {
            bridge,
            _input: input_stream,
            _output: output_stream,
        })
    }

    /// Handle for the UI to read status and the input level meter.
    pub fn monitor(&self) -> AudioMonitor {
        AudioMonitor(Arc::clone(&self.bridge.stats))
    }
}

impl MediaIo for DeviceMedia {
    fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
        self.bridge.capture(frame)
    }
    fn play(&mut self, frame: &[i16; FRAME_SAMPLES]) {
        self.bridge.play(frame);
    }
}

/// Devices opened only while a call's media runs ([`MediaIo::start`] /
/// [`MediaIo::stop`]): no capture before or between calls, and the devices
/// are released when a call ends.
pub struct OnDemandMedia {
    input: Option<String>,
    output: Option<String>,
    open: Option<DeviceMedia>,
    /// Calls whose media currently use the devices (overlaps are possible
    /// when a call's media restarts).
    users: u32,
    observer: Option<Arc<dyn Fn(bool) + Send + Sync>>,
}
impl OnDemandMedia {
    /// Remember the selection; nothing is opened yet.
    #[must_use]
    pub fn new(input: Option<String>, output: Option<String>) -> Self {
        Self {
            input,
            output,
            open: None,
            users: 0,
            observer: None,
        }
    }

    /// Call `observer(true)` when the devices (microphone included) really
    /// open and `observer(false)` when they are released.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn Fn(bool) + Send + Sync>) -> Self {
        self.observer = Some(observer);
        self
    }
    /// Whether the devices are open now.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open.is_some()
    }
}
impl MediaIo for OnDemandMedia {
    fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
        self.open.as_mut().is_some_and(|media| media.capture(frame))
    }
    fn play(&mut self, frame: &[i16; FRAME_SAMPLES]) {
        if let Some(media) = self.open.as_mut() {
            media.play(frame);
        }
    }
    fn start(&mut self) -> bool {
        self.users += 1;
        if self.open.is_none() {
            self.open = DeviceMedia::open(self.input.as_deref(), self.output.as_deref()).ok();
            if self.open.is_some()
                && let Some(observer) = &self.observer
            {
                observer(true);
            }
        }
        self.open.is_some()
    }
    fn stop(&mut self) {
        self.users = self.users.saturating_sub(1);
        if self.users == 0
            && self.open.take().is_some()
            && let Some(observer) = &self.observer
        {
            observer(false);
        }
    }
}

fn matches(device: &Device, selection: &str) -> bool {
    device.id().is_ok_and(|id| id.to_string() == selection) || device.to_string() == selection
}

fn on_stream_error(stats: &Stats, error: &cpal::Error) {
    // A rerouted default stream keeps running; everything else is fatal.
    if error.kind() != cpal::ErrorKind::DeviceChanged {
        stats.stream_failed.store(true, Ordering::Relaxed);
    }
}

fn build_input<T>(
    device: &Device,
    config: StreamConfig,
    mut tx: Producer,
    stats: &Arc<Stats>,
) -> Result<Stream, AudioError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let (data_stats, err_stats) = (Arc::clone(stats), Arc::clone(stats));
    Ok(device.build_input_stream::<T, _, _>(
        config,
        move |data, _| capture_callback(data, channels, &mut tx, &data_stats),
        move |e| on_stream_error(&err_stats, &e),
        Some(OPEN_TIMEOUT),
    )?)
}

fn build_output<T>(
    device: &Device,
    config: StreamConfig,
    mut rx: Consumer,
    mut state: PlaybackState,
    stats: &Arc<Stats>,
) -> Result<Stream, AudioError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let (data_stats, err_stats) = (Arc::clone(stats), Arc::clone(stats));
    Ok(device.build_output_stream::<T, _, _>(
        config,
        move |data, _| playback_callback(data, channels, &mut rx, &mut state, &data_stats),
        move |e| on_stream_error(&err_stats, &e),
        Some(OPEN_TIMEOUT),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge(device_rate: u32) -> (Bridge, Producer, Consumer, Arc<Stats>) {
        let stats = Arc::new(Stats::default());
        let (cap_tx, cap_rx) = ring(ms_samples(device_rate, RING_MS));
        let (play_tx, play_rx) = ring(ms_samples(device_rate, RING_MS));
        (
            Bridge::new(
                cap_rx,
                device_rate,
                play_tx,
                device_rate,
                Arc::clone(&stats),
            ),
            cap_tx,
            play_rx,
            stats,
        )
    }

    #[test]
    fn sound_servers_hide_raw_alsa_devices() {
        let info = |id: &str| DeviceInfo {
            id: id.to_owned(),
            name: id.to_owned(),
            is_default: false,
        };
        let ids = |list: Vec<DeviceInfo>| list.into_iter().map(|d| d.id).collect::<Vec<_>>();
        let raw = vec![
            info("alsa:default"),
            info("alsa:hw:CARD=5,DEV=0"),
            info("alsa:usbstream:CARD=Adapter"),
        ];
        assert_eq!(
            ids(routed_only(raw.clone())).len(),
            3,
            "plain ALSA keeps everything"
        );
        let mut served = raw;
        served.push(info("alsa:pulse"));
        assert_eq!(ids(routed_only(served)), ["alsa:default", "alsa:pulse"]);
    }

    #[test]
    fn sample_conversion_clamps_and_rejects_nan() {
        assert_eq!(f32_to_i16(0.0), 0);
        assert_eq!(f32_to_i16(2.0), i16::MAX);
        assert_eq!(f32_to_i16(-2.0), i16::MIN);
        assert_eq!(f32_to_i16(f32::NAN), 0);
        assert_eq!(f32_to_i16(i16_to_f32(-1234)), -1234);
    }

    #[test]
    fn capture_callback_downmixes_and_counts_overflow() {
        let stats = Stats::default();
        let (mut tx, mut rx) = ring(2);
        // Stereo f32: frames (0.5, -0.5) -> 0, (1.0, 0.0) -> 0.5, third frame overflows.
        capture_callback(&[0.5_f32, -0.5, 1.0, 0.0, 0.2, 0.2], 2, &mut tx, &stats);
        assert_eq!(rx.pop(), Some(0));
        assert_eq!(rx.pop(), Some(16_384));
        assert_eq!(rx.pop(), None);
        assert_eq!(stats.capture_dropped.load(Ordering::Relaxed), 1);
        // Mono i16 passes through unchanged.
        capture_callback(&[i16::MIN, 7], 1, &mut tx, &stats);
        assert_eq!((rx.pop(), rx.pop()), (Some(i16::MIN), Some(7)));
    }

    #[test]
    fn playback_callback_primes_upmixes_and_counts_underruns() {
        let stats = Stats::default();
        let (mut tx, mut rx) = ring(16);
        let mut state = PlaybackState {
            primed: false,
            prime_level: 3,
        };
        let mut out = [1_i16; 6];
        assert!(tx.push(100));
        playback_callback(&mut out, 2, &mut rx, &mut state, &stats);
        assert_eq!(out, [0; 6], "silence until primed");
        assert!(tx.push(200) && tx.push(300));
        playback_callback(&mut out, 2, &mut rx, &mut state, &stats);
        assert_eq!(out, [100, 100, 200, 200, 300, 300]);
        assert_eq!(stats.playback_underruns.load(Ordering::Relaxed), 0);
        let mut out = [0.5_f32; 4];
        assert!(tx.push(16_384));
        playback_callback(&mut out, 2, &mut rx, &mut state, &stats);
        assert_eq!(out, [0.5, 0.5, 0.0, 0.0]);
        assert_eq!(stats.playback_underruns.load(Ordering::Relaxed), 1);
        assert!(!state.primed, "underrun re-primes");
    }

    #[test]
    fn capture_path_bridges_48k_stereo_tone_to_8k_frames() {
        let (mut bridge, mut cap_tx, _, stats) = bridge(48_000);
        let mut frame = [0_i16; FRAME_SAMPLES];
        assert!(!bridge.capture(&mut frame), "nothing captured yet");
        let mut n = 0_u64;
        let mut frames = Vec::new();
        for _ in 0..50 {
            // One 20 ms device callback of stereo f32, then one engine tick.
            let mut buf = [0.0_f32; 960 * 2];
            for pair in buf.as_chunks_mut::<2>().0 {
                let v = (0.5 * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / 48_000.0).sin())
                    as f32;
                pair.fill(v);
                n += 1;
            }
            capture_callback(&buf, 2, &mut cap_tx, &stats);
            if bridge.capture(&mut frame) {
                frames.extend_from_slice(&frame);
            }
        }
        assert!(frames.len() >= 48 * FRAME_SAMPLES, "{}", frames.len());
        let tail = &frames[FRAME_SAMPLES..];
        let peak = tail.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        assert!((16_000..=16_700).contains(&peak), "peak {peak}");
        let crossings = (1..tail.len())
            .filter(|&i| tail[i - 1] < 0 && tail[i] >= 0)
            .count();
        let expected = tail.len() / 8; // 1 kHz at 8 kHz: one cycle per 8 samples
        assert!(
            crossings.abs_diff(expected) <= 2,
            "{crossings} vs {expected}"
        );
        let status = AudioMonitor(stats).status();
        assert_eq!(status.capture_dropped, 0);
        assert!((16_000..=16_700).contains(&status.input_peak));
    }

    #[test]
    fn capture_backlog_is_bounded_by_dropping_oldest() {
        let (mut bridge, mut cap_tx, _, stats) = bridge(8_000);
        for i in 0..1_600 {
            assert!(cap_tx.push(i as i16));
        }
        let mut frame = [0_i16; FRAME_SAMPLES];
        assert!(bridge.capture(&mut frame));
        assert!(bridge.capture_queue.len() <= CAPTURE_QUEUE_MAX);
        assert!(stats.capture_dropped.load(Ordering::Relaxed) > 0);
        // Newest audio is kept: the frame comes from the end of the input.
        assert!(frame[0] > 1_000, "stale frame start {}", frame[0]);
    }

    #[test]
    fn playback_path_upsamples_and_drops_when_backlog_full() {
        let (mut bridge, _, mut play_rx, stats) = bridge(48_000);
        let frame = [1_000_i16; FRAME_SAMPLES];
        for _ in 0..10 {
            bridge.play(&frame);
        }
        // 120 ms at 48 kHz = 5760 samples: frames beyond it are dropped whole.
        let queued = play_rx.len();
        assert!((5_760..=5_760 + 960).contains(&queued), "{queued}");
        assert!(stats.playback_dropped.load(Ordering::Relaxed) >= FRAME_SAMPLES as u64);
        let samples: Vec<i16> = std::iter::from_fn(|| play_rx.pop()).collect();
        assert!(
            samples[200..].iter().all(|s| (s - 1_000).abs() <= 1),
            "DC preserved after warm-up"
        );
    }

    #[test]
    fn device_media_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<DeviceMedia>();
    }

    /// Opens real devices; run manually on a workstation only.
    #[test]
    #[ignore = "requires real audio hardware"]
    fn opens_default_devices() {
        match DeviceMedia::open(None, None) {
            Ok(media) => assert!(!media.monitor().status().stream_failed),
            Err(error) => panic!("open failed: {error}"),
        }
    }

    #[test]
    fn on_demand_media_opens_nothing_until_a_call_starts() {
        use rust_microsip_core::MediaIo as _;
        let mut media = super::OnDemandMedia::new(Some("no-such-device".into()), None);
        assert!(!media.is_open());
        let mut frame = [0i16; rust_microsip_core::FRAME_SAMPLES];
        assert!(!media.capture(&mut frame), "silent while closed");
        // The named device does not exist: start reports it and stays closed.
        assert!(!media.start());
        assert!(!media.is_open());
        media.stop();
        assert!(!media.is_open());
    }
}
