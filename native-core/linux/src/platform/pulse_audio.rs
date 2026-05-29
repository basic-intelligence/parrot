#[cfg(target_os = "linux")]
mod imp {
    use anyhow::{anyhow, Context};
    use parrot_audio::RecordedAudio;
    use parrot_protocol::AudioDevice;
    use std::{
        process::Command,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread,
    };

    const SAMPLE_RATE_HZ: u32 = 16_000;

    #[derive(Debug, Clone)]
    struct PulseSource {
        name: String,
        description: String,
        is_default: bool,
    }

    #[derive(Clone, Default)]
    pub struct PulseAudioManager {
        active: Arc<Mutex<Option<ActivePulseRecording>>>,
    }

    struct ActivePulseRecording {
        stop: Arc<AtomicBool>,
        join: thread::JoinHandle<anyhow::Result<Vec<f32>>>,
    }

    impl std::fmt::Debug for PulseAudioManager {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("PulseAudioManager")
        }
    }

    impl PulseAudioManager {
        pub fn available() -> bool {
            Command::new("pactl")
                .args(["info"])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }

        pub fn probe_input_device(&self, selected_uid: Option<&str>) -> anyhow::Result<()> {
            let selected_uid = selected_uid
                .and_then(|uid| uid.strip_prefix("pulse:"))
                .filter(|uid| !uid.is_empty() && *uid != "default");
            let devices = self.list_input_devices()?;

            if let Some(selected_uid) = selected_uid {
                if !devices
                    .iter()
                    .any(|device| device.uid == format!("pulse:{selected_uid}"))
                {
                    return Err(anyhow!(
                        "Selected PulseAudio/PipeWire source is not available: {selected_uid}"
                    ));
                }
            }

            Ok(())
        }

        pub fn is_recording(&self) -> bool {
            self.active
                .lock()
                .expect("pulse recorder poisoned")
                .is_some()
        }

        pub fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
            let sources = list_sources()?;

            let mut devices = vec![AudioDevice {
                uid: "pulse:default".into(),
                name: "System Default".into(),
                is_default: true,
            }];

            devices.extend(sources.into_iter().map(|source| AudioDevice {
                uid: format!("pulse:{}", source.name),
                name: source.description,
                is_default: source.is_default,
            }));

            Ok(devices)
        }

        pub fn start_recording(&self, selected_uid: Option<&str>) -> anyhow::Result<()> {
            let mut active = self.active.lock().expect("pulse recorder poisoned");
            if active.is_some() {
                return Err(anyhow!("audio recording is already active"));
            }

            let source_name = selected_uid
                .and_then(|uid| uid.strip_prefix("pulse:"))
                .filter(|value| *value != "default")
                .map(str::to_string);

            let stop = Arc::new(AtomicBool::new(false));
            let stop_for_thread = Arc::clone(&stop);

            let join = thread::Builder::new()
                .name("Parrot PulseAudio recorder".into())
                .spawn(move || record_pulse_source(source_name.as_deref(), stop_for_thread))
                .context("failed to start PulseAudio recording thread")?;

            *active = Some(ActivePulseRecording { stop, join });

            Ok(())
        }

        pub fn stop_recording(&self) -> anyhow::Result<RecordedAudio> {
            let active = self
                .active
                .lock()
                .expect("pulse recorder poisoned")
                .take()
                .context("audio recording is not active")?;

            active.stop.store(true, Ordering::SeqCst);
            let samples = active
                .join
                .join()
                .map_err(|_| anyhow!("PulseAudio recording thread panicked"))??;

            Ok(RecordedAudio {
                samples,
                sample_rate_hz: SAMPLE_RATE_HZ,
                channels: 1,
            })
        }
    }

    fn default_source_name() -> Option<String> {
        let output = Command::new("pactl")
            .args(["get-default-source"])
            .output()
            .ok()?;

        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn list_sources() -> anyhow::Result<Vec<PulseSource>> {
        let default = default_source_name();
        let output = Command::new("pactl")
            .args(["list", "sources", "short"])
            .output()
            .context("failed to run pactl list sources short")?;

        if !output.status.success() {
            return Err(anyhow!("pactl list sources short failed"));
        }

        Ok(list_sources_from_short_output(
            &String::from_utf8_lossy(&output.stdout),
            default.as_deref(),
        ))
    }

    fn list_sources_from_short_output(text: &str, default: Option<&str>) -> Vec<PulseSource> {
        let mut sources = Vec::new();

        for line in text.lines() {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() < 2 {
                continue;
            }

            let name = fields[1].trim();
            if name.is_empty() || name.ends_with(".monitor") {
                continue;
            }

            sources.push(PulseSource {
                name: name.to_string(),
                description: name.to_string(),
                is_default: default == Some(name),
            });
        }

        sources
    }

    fn record_pulse_source(
        source_name: Option<&str>,
        stop: Arc<AtomicBool>,
    ) -> anyhow::Result<Vec<f32>> {
        use libpulse_binding::{
            def::BufferAttr,
            sample::{Format, Spec},
            stream::Direction,
        };
        use libpulse_simple_binding::Simple;

        let spec = Spec {
            format: Format::S16le,
            channels: 1,
            rate: SAMPLE_RATE_HZ,
        };

        if !spec.is_valid() {
            return Err(anyhow!("invalid PulseAudio recording format"));
        }

        let attr = BufferAttr {
            maxlength: u32::MAX,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: 4096,
        };

        let recorder = Simple::new(
            None,
            "Parrot",
            Direction::Record,
            source_name,
            "Dictation",
            &spec,
            None,
            Some(&attr),
        )
        .map_err(|error| anyhow!("failed to open PulseAudio/PipeWire input source: {error}"))?;

        let mut bytes = [0_u8; 4096];
        let mut samples = Vec::<f32>::new();

        while !stop.load(Ordering::SeqCst) {
            recorder
                .read(&mut bytes)
                .map_err(|error| anyhow!("failed to read PulseAudio/PipeWire input: {error}"))?;

            for chunk in bytes.chunks_exact(2) {
                let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                samples.push(sample as f32 / i16::MAX as f32);
            }
        }

        Ok(samples)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_sources_and_skips_monitors() {
            let output = "\
45\talsa_input.pci-0000_00_1f.3.analog-stereo\tPipeWire\tfloat32le 2ch 48000Hz\tRUNNING
46\talsa_output.pci-0000_00_1f.3.analog-stereo.monitor\tPipeWire\tfloat32le 2ch 48000Hz\tIDLE
";

            let sources = list_sources_from_short_output(
                output,
                Some("alsa_input.pci-0000_00_1f.3.analog-stereo"),
            );

            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].name, "alsa_input.pci-0000_00_1f.3.analog-stereo");
            assert!(sources[0].is_default);
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use anyhow::anyhow;
    use parrot_audio::RecordedAudio;
    use parrot_protocol::AudioDevice;

    #[derive(Clone, Default)]
    pub struct PulseAudioManager;

    impl std::fmt::Debug for PulseAudioManager {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("PulseAudioManager")
        }
    }

    impl PulseAudioManager {
        pub fn available() -> bool {
            false
        }

        pub fn probe_input_device(&self, _selected_uid: Option<&str>) -> anyhow::Result<()> {
            Err(anyhow!("PulseAudio is only available on Linux"))
        }

        pub fn is_recording(&self) -> bool {
            false
        }

        pub fn list_input_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
            Err(anyhow!("PulseAudio is only available on Linux"))
        }

        pub fn start_recording(&self, _selected_uid: Option<&str>) -> anyhow::Result<()> {
            Err(anyhow!("PulseAudio is only available on Linux"))
        }

        pub fn stop_recording(&self) -> anyhow::Result<RecordedAudio> {
            Err(anyhow!("PulseAudio is only available on Linux"))
        }
    }
}

pub use imp::*;
