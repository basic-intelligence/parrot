use crate::platform::pulse_audio::PulseAudioManager;
use anyhow::{anyhow, Context};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, Stream, StreamConfig,
};
use parrot_audio::RecordedAudio;
use parrot_protocol::AudioDevice;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;

#[derive(Clone, Default)]
pub struct AudioManager {
    pulse: PulseAudioManager,
    cpal: CpalAudioManager,
}

#[derive(Clone, Default)]
struct CpalAudioManager {
    recording: Arc<Mutex<Option<ActiveRecording>>>,
}

// SAFETY: Linux recording streams are created, played, and dropped through the
// mutex-protected AudioManager API. The stream callbacks only append samples to
// Arc<Mutex<_>> buffers and do not share thread-affine UI state.
unsafe impl Send for CpalAudioManager {}

// SAFETY: See the Send impl above; all mutable stream access is synchronized.
unsafe impl Sync for CpalAudioManager {}

pub(crate) fn probe_input_device(selected_uid: Option<&str>) -> anyhow::Result<()> {
    let pulse_available = PulseAudioManager::available();
    if pulse_available {
        return PulseAudioManager::default().probe_input_device(selected_uid_for_pulse_backend(
            pulse_available,
            selected_uid,
        ));
    }

    let entry = select_cpal_input_device(selected_uid)?;
    entry
        .device
        .default_input_config()
        .map_err(|error| microphone_capture_error("probe", &entry.name, error))?;
    Ok(())
}

struct ActiveRecording {
    _stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    stream_error: Arc<Mutex<Option<String>>>,
    input_sample_rate: u32,
}

impl std::fmt::Debug for AudioManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AudioManager")
    }
}

impl std::fmt::Debug for CpalAudioManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CpalAudioManager")
    }
}

struct InputDeviceEntry {
    device: Device,
    uid: String,
    name: String,
    is_default: bool,
}

impl AudioManager {
    pub fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
        if PulseAudioManager::available() {
            if let Ok(devices) = self.pulse.list_input_devices() {
                if !devices.is_empty() {
                    return Ok(devices);
                }
            }
        }

        self.cpal.list_input_devices()
    }

    pub fn start_recording(&self, selected_uid: Option<&str>) -> anyhow::Result<()> {
        let pulse_available = PulseAudioManager::available();
        if pulse_available {
            return self.pulse.start_recording(selected_uid_for_pulse_backend(
                pulse_available,
                selected_uid,
            ));
        }

        self.cpal.start_recording(selected_uid)
    }

    pub fn stop_recording(&self) -> anyhow::Result<RecordedAudio> {
        if self.pulse.is_recording() {
            let audio = self.pulse.stop_recording()?;
            log_audio_capture(
                "PulseAudio/PipeWire",
                &audio.samples,
                audio.sample_rate_hz,
                audio.sample_rate_hz,
            );
            maybe_write_debug_wav(&audio.samples, audio.sample_rate_hz);
            return Ok(audio);
        }

        self.cpal.stop_recording()
    }
}

impl CpalAudioManager {
    fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
        Ok(cpal_input_device_entries()?
            .into_iter()
            .map(|entry| AudioDevice {
                uid: entry.uid,
                name: entry.name,
                is_default: entry.is_default,
            })
            .collect())
    }

    fn start_recording(&self, selected_uid: Option<&str>) -> anyhow::Result<()> {
        let mut recording = self.recording.lock().expect("audio recorder poisoned");
        if recording.is_some() {
            return Err(anyhow!("audio recording is already active"));
        }

        let entry = select_cpal_input_device(selected_uid)?;
        let supported_config = entry.device.default_input_config().map_err(|error| {
            microphone_capture_error("read input format for", &entry.name, error)
        })?;
        let sample_format = supported_config.sample_format();
        let stream_config: StreamConfig = supported_config.into();
        let sample_rate = stream_config.sample_rate.0;
        let channels = stream_config.channels;
        let samples = Arc::new(Mutex::new(Vec::new()));
        let stream_error = Arc::new(Mutex::new(None));
        let error_label = entry.name.clone();
        let error_for_callback = stream_error.clone();
        let error_callback = move |error: cpal::StreamError| {
            let message = microphone_stream_error_message(&error_label, &error.to_string());
            eprintln!("{message}");
            *error_for_callback
                .lock()
                .expect("audio stream error poisoned") = Some(message);
        };

        let stream = match sample_format {
            SampleFormat::F32 => build_input_stream::<f32, _>(
                &entry.device,
                &stream_config,
                channels,
                samples.clone(),
                |sample| sample.clamp(-1.0, 1.0),
                error_callback,
            ),
            SampleFormat::I16 => build_input_stream::<i16, _>(
                &entry.device,
                &stream_config,
                channels,
                samples.clone(),
                i16_to_f32,
                error_callback,
            ),
            SampleFormat::U16 => build_input_stream::<u16, _>(
                &entry.device,
                &stream_config,
                channels,
                samples.clone(),
                u16_to_f32,
                error_callback,
            ),
            other => {
                return Err(anyhow!(
                    "unsupported Linux microphone sample format: {other:?}"
                ))
            }
        }
        .map_err(|error| microphone_capture_error("open", &entry.name, error))?;

        stream
            .play()
            .map_err(|error| microphone_capture_error("start", &entry.name, error))?;
        *recording = Some(ActiveRecording {
            _stream: stream,
            samples,
            stream_error,
            input_sample_rate: sample_rate,
        });
        Ok(())
    }

    fn stop_recording(&self) -> anyhow::Result<RecordedAudio> {
        let active = self
            .recording
            .lock()
            .expect("audio recorder poisoned")
            .take()
            .context("audio recording is not active")?;
        drop(active._stream);

        if let Some(error) = active
            .stream_error
            .lock()
            .expect("audio stream error poisoned")
            .clone()
        {
            return Err(anyhow!(error));
        }

        let raw_samples = active
            .samples
            .lock()
            .expect("audio samples poisoned")
            .clone();
        let samples = resample_linear_to_16khz(&raw_samples, active.input_sample_rate);
        log_audio_capture(
            "CPAL",
            &samples,
            TARGET_SAMPLE_RATE_HZ,
            active.input_sample_rate,
        );
        maybe_write_debug_wav(&samples, TARGET_SAMPLE_RATE_HZ);

        Ok(RecordedAudio {
            samples,
            sample_rate_hz: TARGET_SAMPLE_RATE_HZ,
            channels: 1,
        })
    }
}

fn build_input_stream<T, F>(
    device: &Device,
    config: &StreamConfig,
    channels: u16,
    samples: Arc<Mutex<Vec<f32>>>,
    convert: F,
    error_callback: impl FnMut(cpal::StreamError) + Send + 'static,
) -> anyhow::Result<Stream>
where
    T: cpal::SizedSample,
    F: Fn(T) -> f32 + Send + Copy + 'static,
{
    let channel_count = usize::from(channels.max(1));
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mono = interleaved_to_mono(data, channel_count, convert);
                if !mono.is_empty() {
                    samples.lock().expect("audio samples poisoned").extend(mono);
                }
            },
            error_callback,
            None,
        )
        .context("failed to build Linux microphone stream")
}

fn microphone_capture_error(
    action: &str,
    device_name: &str,
    error: impl std::fmt::Display,
) -> anyhow::Error {
    let message = error.to_string();
    if is_microphone_privacy_denial(&message) {
        return anyhow!(
            "Linux microphone access is blocked. Check your desktop privacy settings, PipeWire/PulseAudio input permissions, and selected input device, then try again."
        );
    }

    anyhow!("Could not {action} Linux microphone `{device_name}`: {message}")
}

fn microphone_stream_error_message(device_name: &str, error: &str) -> String {
    if is_microphone_privacy_denial(error) {
        return "Linux microphone access was blocked while recording. Check your desktop privacy settings, PipeWire/PulseAudio input permissions, and selected input device, then try again.".into();
    }

    format!(
        "Linux microphone `{device_name}` stopped while recording. Check that the microphone is still connected and selected, then try again. Details: {error}"
    )
}

fn is_microphone_privacy_denial(message: &str) -> bool {
    let normalized = message.to_lowercase();
    [
        "access denied",
        "permission denied",
        "privacy",
        "not authorized",
        "unauthorized",
        "denied by system",
        "device access is denied",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn cpal_input_device_entries() -> anyhow::Result<Vec<InputDeviceEntry>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let mut default_claimed = false;
    let mut name_counts = HashMap::<String, usize>::new();
    let mut entries = Vec::new();

    for device in host
        .input_devices()
        .context("failed to enumerate Linux input devices")?
    {
        let name = device
            .name()
            .unwrap_or_else(|_| "Unknown microphone".into());
        let occurrence = name_counts.entry(name.clone()).or_insert(0);
        let uid = stable_device_uid(&name, *occurrence);
        *occurrence += 1;
        let is_default = default_name.as_deref() == Some(name.as_str()) && !default_claimed;
        if is_default {
            default_claimed = true;
        }
        entries.push(InputDeviceEntry {
            device,
            uid,
            name,
            is_default,
        });
    }

    Ok(entries)
}

fn select_cpal_input_device(selected_uid: Option<&str>) -> anyhow::Result<InputDeviceEntry> {
    let entries = cpal_input_device_entries()?;
    if entries.is_empty() {
        return Err(anyhow!("No Linux microphone input devices were found."));
    }

    let index = selected_device_index(
        &entries
            .iter()
            .map(|entry| (entry.uid.as_str(), entry.is_default))
            .collect::<Vec<_>>(),
        selected_uid,
    );
    Ok(entries
        .into_iter()
        .nth(index)
        .expect("selected index must exist"))
}

fn selected_device_index(entries: &[(&str, bool)], selected_uid: Option<&str>) -> usize {
    if let Some(selected_uid) = selected_uid.filter(|uid| !uid.is_empty()) {
        if let Some(index) = entries.iter().position(|(uid, _)| *uid == selected_uid) {
            return index;
        }
    }

    entries
        .iter()
        .position(|(_, is_default)| *is_default)
        .unwrap_or(0)
}

fn selected_uid_for_pulse_backend<'a>(
    pulse_available: bool,
    selected_uid: Option<&'a str>,
) -> Option<&'a str> {
    if pulse_available
        && selected_uid
            .map(|uid| uid.starts_with("cpal:"))
            .unwrap_or(false)
    {
        None
    } else {
        selected_uid
    }
}

fn log_audio_capture(
    backend: &str,
    samples: &[f32],
    sample_rate_hz: u32,
    source_sample_rate_hz: u32,
) {
    let (peak, rms) = audio_stats(samples);

    eprintln!(
        "Linux audio captured via {backend}: {:.2}s, samples={}, peak={:.5}, rms={:.5}, source_rate={}",
        samples.len() as f64 / sample_rate_hz.max(1) as f64,
        samples.len(),
        peak,
        rms,
        source_sample_rate_hz
    );
}

fn audio_stats(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let peak = samples
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max);
    let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>()
        / samples.len().max(1) as f32)
        .sqrt();

    (peak, rms)
}

fn maybe_write_debug_wav(samples: &[f32], sample_rate_hz: u32) {
    if std::env::var_os("PARROT_DEBUG_AUDIO").is_none() && !cfg!(debug_assertions) {
        return;
    }

    let path = Path::new("/tmp/parrot-last-capture.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let Ok(mut writer) = hound::WavWriter::create(path, spec) else {
        return;
    };

    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        let _ = writer.write_sample(value);
    }

    let _ = writer.finalize();
}

#[cfg(test)]
pub fn selected_uid_exists(devices: &[AudioDevice], selected_uid: &str) -> bool {
    devices.iter().any(|device| device.uid == selected_uid)
}

fn stable_device_uid(name: &str, occurrence: usize) -> String {
    let normalized = name.trim().to_lowercase();
    format!("cpal:{:016x}:{occurrence}", fnv1a64(normalized.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn interleaved_to_mono<T, F>(samples: &[T], channels: usize, convert: F) -> Vec<f32>
where
    T: Copy,
    F: Fn(T) -> f32 + Copy,
{
    let channel_count = channels.max(1);
    let mut mono = Vec::with_capacity(samples.len() / channel_count);
    for frame in samples.chunks(channel_count) {
        let sum = frame.iter().copied().map(convert).sum::<f32>();
        mono.push((sum / frame.len() as f32).clamp(-1.0, 1.0));
    }
    mono
}

fn i16_to_f32(sample: i16) -> f32 {
    if sample == i16::MIN {
        -1.0
    } else {
        f32::from(sample) / f32::from(i16::MAX)
    }
}

fn u16_to_f32(sample: u16) -> f32 {
    (sample as f32 / u16::MAX as f32) * 2.0 - 1.0
}

fn resample_linear_to_16khz(samples: &[f32], source_sample_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    if source_sample_rate == TARGET_SAMPLE_RATE_HZ {
        return samples.to_vec();
    }
    if source_sample_rate == 0 {
        return Vec::new();
    }

    let ratio = TARGET_SAMPLE_RATE_HZ as f64 / source_sample_rate as f64;
    let output_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let source_step = source_sample_rate as f64 / TARGET_SAMPLE_RATE_HZ as f64;
    let last = samples.len() - 1;
    let mut output = Vec::with_capacity(output_len);

    for output_index in 0..output_len {
        let source_pos = output_index as f64 * source_step;
        let left = source_pos.floor() as usize;
        let right = (left + 1).min(last);
        let fraction = (source_pos - left as f64) as f32;
        let sample = if left >= last {
            samples[last]
        } else {
            samples[left] + (samples[right] - samples[left]) * fraction
        };
        output.push(sample.clamp(-1.0, 1.0));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_uid_is_deterministic_and_distinguishes_duplicates() {
        assert_eq!(
            stable_device_uid("USB Mic", 0),
            stable_device_uid("usb mic", 0)
        );
        assert_ne!(
            stable_device_uid("USB Mic", 0),
            stable_device_uid("USB Mic", 1)
        );
    }

    #[test]
    fn converts_interleaved_i16_to_mono_f32() {
        let mono = interleaved_to_mono(&[i16::MAX, i16::MAX, i16::MIN, i16::MIN], 2, i16_to_f32);

        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 1.0).abs() < 0.0001);
        assert!((mono[1] + 1.0).abs() < 0.0001);
    }

    #[test]
    fn resamples_to_16khz_with_linear_interpolation() {
        let output = resample_linear_to_16khz(&[0.0, 1.0], 8_000);

        assert_eq!(output.len(), 4);
        assert_eq!(output[0], 0.0);
        assert!(output[1] > 0.0 && output[1] < 1.0);
        assert_eq!(output[3], 1.0);
    }

    #[test]
    fn selected_device_falls_back_to_default_then_first_device() {
        let entries = [("one", false), ("two", true)];
        assert_eq!(selected_device_index(&entries, Some("missing")), 1);
        assert_eq!(selected_device_index(&entries, Some("one")), 0);

        let entries = [("one", false), ("two", false)];
        assert_eq!(selected_device_index(&entries, Some("missing")), 0);
    }

    #[test]
    fn selected_uid_exists_checks_linux_device_list() {
        let devices = vec![AudioDevice {
            uid: "cpal:one".into(),
            name: "Microphone".into(),
            is_default: true,
        }];

        assert!(selected_uid_exists(&devices, "cpal:one"));
        assert!(!selected_uid_exists(&devices, "macos:stale"));
    }

    #[test]
    fn stale_cpal_uid_is_ignored_when_pulse_is_available() {
        assert_eq!(
            selected_uid_for_pulse_backend(true, Some("cpal:stale")),
            None
        );
        assert_eq!(
            selected_uid_for_pulse_backend(true, Some("pulse:default")),
            Some("pulse:default")
        );
        assert_eq!(
            selected_uid_for_pulse_backend(false, Some("cpal:stale")),
            Some("cpal:stale")
        );
    }

    #[test]
    fn audio_stats_reports_peak_and_rms() {
        let (peak, rms) = audio_stats(&[-0.5, 0.0, 1.0]);

        assert_eq!(peak, 1.0);
        assert!((rms - 0.64549).abs() < 0.0001);
    }

    #[test]
    fn microphone_privacy_errors_include_linux_guidance() {
        let error = microphone_capture_error("start", "USB Mic", "access denied").to_string();

        assert!(error.contains("Linux microphone access is blocked"));
        assert!(error.contains("PipeWire/PulseAudio"));
    }
}
