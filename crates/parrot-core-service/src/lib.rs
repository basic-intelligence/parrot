use async_trait::async_trait;
use parrot_audio::{trim_recorded_audio_for_dictation, RecordedAudio};
use parrot_language::{
    decode_language_code, detected_language_metadata, selected_language_metadata,
    DictationLanguageMetadata, DictationLanguageSettings,
};
use parrot_models::{
    catalog, cleanup_model_for, model_status, required_models, Architecture, ModelDescriptor,
    ModelFileState, Platform,
};
use parrot_prompts::{assemble_cleanup_prompt, CleanupPromptInput};
use parrot_protocol::{
    AppSettings, AudioDevice, ModelStatus, NativeCoreEvent, PermissionKind, PermissionSnapshot,
    RecordingResult, ShortcutSettings, SoundEvent, NATIVE_CORE_EVENT_HOTKEY_MONITOR_FAILED,
    NATIVE_CORE_EVENT_RECORDING_CANCELLED, NATIVE_CORE_EVENT_RECORDING_FAILED,
    NATIVE_CORE_EVENT_RECORDING_FINISHED, NATIVE_CORE_EVENT_RECORDING_PROCESSING,
    NATIVE_CORE_EVENT_RECORDING_STARTED,
};
use parrot_settings::{
    normalize_settings_for_platform, SettingsPlatform, DEFAULT_CLEANUP_MODEL_ID,
};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct ShortcutBindings {
    pub push_to_talk: ShortcutSettings,
    pub hands_free: ShortcutSettings,
}

#[derive(Debug, Clone, Copy)]
pub enum ShortcutTarget {
    PushToTalk,
    HandsFree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteTarget {
    pub platform_id: String,
}

#[derive(Debug, Clone)]
pub enum HotkeyStopRecording {
    Idle,
    Busy(NativeCoreEvent),
    Started {
        event: NativeCoreEvent,
        paste_target: Option<PasteTarget>,
    },
}

#[derive(Debug, Clone)]
pub struct CoreServiceConfig {
    pub settings: AppSettings,
    pub platform: Platform,
    pub architecture: Architecture,
    pub cleanup_default_instructions: String,
    pub debug_cleanup_failures: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionOutput {
    pub text: String,
    pub detected_language_code: Option<String>,
}

#[async_trait]
pub trait PlatformAdapter {
    async fn list_audio_devices(&self) -> anyhow::Result<Vec<AudioDevice>>;
    async fn start_audio_recording(&self, input_uid: Option<&str>) -> anyhow::Result<()>;
    async fn stop_audio_recording(&self) -> anyhow::Result<RecordedAudio>;

    async fn permission_snapshot(&self) -> anyhow::Result<PermissionSnapshot>;
    async fn request_permission(
        &self,
        kind: PermissionKind,
        open_settings: bool,
    ) -> anyhow::Result<PermissionSnapshot>;

    async fn start_hotkey_monitor(&self, shortcuts: ShortcutBindings) -> anyhow::Result<()>;
    async fn stop_hotkey_monitor(&self) -> anyhow::Result<()>;
    async fn capture_shortcut(&self, target: ShortcutTarget) -> anyhow::Result<ShortcutSettings>;

    async fn capture_paste_target(&self) -> anyhow::Result<Option<PasteTarget>>;
    async fn focused_text_before_cursor(
        &self,
        target: Option<&PasteTarget>,
    ) -> anyhow::Result<Option<String>>;
    async fn paste_text(&self, text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()>;

    fn play_sound(&self, event: SoundEvent, enabled: bool);
}

#[async_trait]
pub trait ModelPipeline {
    async fn model_state(&self, descriptor: &ModelDescriptor) -> anyhow::Result<ModelFileState>;
    async fn download_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()>;
    async fn delete_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()>;
    async fn warm_speech_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()>;
    async fn warm_cleanup_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()>;
    async fn transcribe(
        &self,
        descriptor: &ModelDescriptor,
        audio: &RecordedAudio,
        language_code: Option<&str>,
        detect_language: bool,
    ) -> anyhow::Result<TranscriptionOutput>;
    async fn cleanup(
        &self,
        descriptor: &ModelDescriptor,
        prompt: &parrot_prompts::CleanupPrompt,
    ) -> anyhow::Result<String>;
}

pub struct CoreService<A: PlatformAdapter, M: ModelPipeline> {
    adapter: A,
    models: M,
    settings: AppSettings,
    platform: Platform,
    architecture: Architecture,
    cleanup_default_instructions: String,
    debug_cleanup_failures: bool,
    hotkey_recording: bool,
    hotkey_processing: bool,
    paste_target: Option<PasteTarget>,
}

impl<A: PlatformAdapter, M: ModelPipeline> CoreService<A, M> {
    pub fn new(adapter: A, models: M, mut config: CoreServiceConfig) -> Self {
        normalize_settings_for_platform(&mut config.settings, settings_platform(config.platform));
        Self {
            adapter,
            models,
            settings: config.settings,
            platform: config.platform,
            architecture: config.architecture,
            cleanup_default_instructions: config.cleanup_default_instructions,
            debug_cleanup_failures: config.debug_cleanup_failures,
            hotkey_recording: false,
            hotkey_processing: false,
            paste_target: None,
        }
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    pub fn hotkey_recording_active(&self) -> bool {
        self.hotkey_recording
    }

    pub fn hotkey_recording_processing(&self) -> bool {
        self.hotkey_processing
    }

    pub fn update_settings(&mut self, mut settings: AppSettings) -> AppSettings {
        normalize_settings_for_platform(&mut settings, settings_platform(self.platform));
        self.settings = settings.clone();
        settings
    }

    pub async fn permission_snapshot(&self) -> anyhow::Result<PermissionSnapshot> {
        self.adapter.permission_snapshot().await
    }

    pub async fn request_permission(
        &self,
        kind: PermissionKind,
        open_settings: bool,
    ) -> anyhow::Result<PermissionSnapshot> {
        self.adapter.request_permission(kind, open_settings).await
    }

    pub async fn list_audio_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
        self.adapter.list_audio_devices().await
    }

    pub async fn model_statuses(&self) -> anyhow::Result<Vec<ModelStatus>> {
        let required = self.required_descriptors();
        let required_ids = required
            .iter()
            .map(|model| model.public_id.as_str())
            .collect::<Vec<_>>();

        let mut statuses = Vec::new();
        for descriptor in self.catalog_descriptors() {
            let state = self.models.model_state(&descriptor).await?;
            statuses.push(model_status(
                &descriptor,
                state,
                required_ids.contains(&descriptor.public_id.as_str()),
            ));
        }
        Ok(statuses)
    }

    pub async fn download_model(&self, public_id: &str) -> anyhow::Result<Vec<ModelStatus>> {
        let descriptor = self
            .descriptor_for_public_id(public_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {public_id}"))?;
        self.models.download_model(&descriptor).await?;
        self.model_statuses().await
    }

    pub async fn delete_model(&self, public_id: &str) -> anyhow::Result<Vec<ModelStatus>> {
        let descriptor = self
            .descriptor_for_public_id(public_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown model: {public_id}"))?;
        self.models.delete_model(&descriptor).await?;
        self.model_statuses().await
    }

    pub async fn warm_models(&self) -> anyhow::Result<()> {
        for descriptor in self.required_descriptors() {
            match descriptor.role {
                parrot_protocol::ModelRole::Speech => {
                    self.models.warm_speech_model(&descriptor).await?;
                }
                parrot_protocol::ModelRole::Cleanup => {
                    self.models.warm_cleanup_model(&descriptor).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn start_recording(&self) -> anyhow::Result<()> {
        self.adapter
            .start_audio_recording(self.settings.selected_input_uid.as_deref())
            .await?;
        self.adapter
            .play_sound(SoundEvent::RecordingStart, self.settings.play_sounds);
        Ok(())
    }

    pub async fn stop_recording(&self) -> anyhow::Result<RecordingResult> {
        let result = self.finish_recording().await?;
        self.adapter
            .play_sound(SoundEvent::RecordingSuccess, self.settings.play_sounds);
        Ok(result)
    }

    pub async fn start_hotkey_monitor(&self) -> anyhow::Result<()> {
        self.adapter
            .start_hotkey_monitor(ShortcutBindings {
                push_to_talk: self.settings.push_to_talk_shortcut.clone(),
                hands_free: self.settings.hands_free_shortcut.clone(),
            })
            .await
    }

    pub async fn stop_hotkey_monitor(&self) -> anyhow::Result<()> {
        self.adapter.stop_hotkey_monitor().await
    }

    pub async fn capture_shortcut(
        &self,
        target: ShortcutTarget,
    ) -> anyhow::Result<ShortcutSettings> {
        self.adapter.capture_shortcut(target).await
    }

    pub async fn start_hotkey_recording(&mut self) -> NativeCoreEvent {
        if self.hotkey_recording {
            return event(
                NATIVE_CORE_EVENT_RECORDING_STARTED,
                json!({ "kind": "dictation", "busy": true }),
            );
        }

        if self.hotkey_processing {
            return event(
                NATIVE_CORE_EVENT_RECORDING_PROCESSING,
                json!({ "kind": "dictation", "busy": true }),
            );
        }

        self.paste_target = if self.should_capture_hotkey_paste_target() {
            self.adapter.capture_paste_target().await.ok().flatten()
        } else {
            None
        };

        match self.start_recording().await {
            Ok(()) => {
                self.hotkey_recording = true;
                event(
                    NATIVE_CORE_EVENT_RECORDING_STARTED,
                    json!({ "kind": "dictation" }),
                )
            }
            Err(error) => {
                self.paste_target = None;
                self.hotkey_recording = false;
                self.hotkey_processing = false;
                self.adapter
                    .play_sound(SoundEvent::RecordingError, self.settings.play_sounds);
                event(
                    NATIVE_CORE_EVENT_RECORDING_FAILED,
                    json!({ "error": error.to_string() }),
                )
            }
        }
    }

    pub async fn stop_hotkey_recording(&mut self) -> Vec<NativeCoreEvent> {
        match self.begin_stop_hotkey_recording() {
            HotkeyStopRecording::Idle => Vec::new(),
            HotkeyStopRecording::Busy(event) => vec![event],
            HotkeyStopRecording::Started {
                event,
                paste_target,
            } => {
                let mut events = vec![event];
                events.push(self.finish_stopped_hotkey_recording(paste_target).await);
                events
            }
        }
    }

    pub fn begin_stop_hotkey_recording(&mut self) -> HotkeyStopRecording {
        if self.hotkey_processing {
            return HotkeyStopRecording::Busy(event(
                NATIVE_CORE_EVENT_RECORDING_PROCESSING,
                json!({ "kind": "dictation", "busy": true }),
            ));
        }

        if !self.hotkey_recording {
            return HotkeyStopRecording::Idle;
        }

        self.hotkey_recording = false;
        self.hotkey_processing = true;
        let paste_target = self.paste_target.take();

        HotkeyStopRecording::Started {
            event: event(
                NATIVE_CORE_EVENT_RECORDING_PROCESSING,
                json!({ "kind": "dictation" }),
            ),
            paste_target,
        }
    }

    pub async fn finish_stopped_hotkey_recording(
        &mut self,
        paste_target: Option<PasteTarget>,
    ) -> NativeCoreEvent {
        let event = match self.finish_recording().await {
            Ok(result) => {
                let paste_error = self
                    .paste_recording_result(&result, paste_target.as_ref())
                    .await;
                if paste_error.is_none() {
                    self.adapter
                        .play_sound(SoundEvent::RecordingSuccess, self.settings.play_sounds);
                } else {
                    self.adapter
                        .play_sound(SoundEvent::RecordingError, self.settings.play_sounds);
                }

                let mut payload = serde_json::to_value(&result).unwrap_or_else(|_| json!({}));
                payload["kind"] = json!("dictation");
                if let Some(error) = paste_error {
                    payload["pasteError"] = json!(error);
                }
                event(NATIVE_CORE_EVENT_RECORDING_FINISHED, payload)
            }
            Err(error) => {
                self.adapter
                    .play_sound(SoundEvent::RecordingError, self.settings.play_sounds);
                event(
                    NATIVE_CORE_EVENT_RECORDING_FAILED,
                    json!({ "error": error.to_string() }),
                )
            }
        };

        self.hotkey_processing = false;
        event
    }

    pub async fn cancel_hotkey_recording(&mut self) -> NativeCoreEvent {
        if self.hotkey_recording {
            let _ = self.adapter.stop_audio_recording().await;
        }
        self.hotkey_recording = false;
        self.hotkey_processing = false;
        self.paste_target = None;
        self.adapter
            .play_sound(SoundEvent::RecordingCancel, self.settings.play_sounds);
        event(
            NATIVE_CORE_EVENT_RECORDING_CANCELLED,
            json!({ "kind": "dictation" }),
        )
    }

    async fn finish_recording(&self) -> anyhow::Result<RecordingResult> {
        let raw_audio = self.adapter.stop_audio_recording().await?;
        let audio_duration_seconds = duration_seconds(&raw_audio);
        let trimmed_audio = trim_recorded_audio_for_dictation(&raw_audio);
        if trimmed_audio.samples.is_empty() {
            return Err(anyhow::anyhow!("No speech detected."));
        }

        let language_settings = DictationLanguageSettings::from(&self.settings);
        let speech_descriptor =
            required_models(&language_settings, self.platform, self.architecture)
                .into_iter()
                .find(|model| model.role == parrot_protocol::ModelRole::Speech)
                .ok_or_else(|| {
                    anyhow::anyhow!("No speech model is available for this platform.")
                })?;
        self.require_downloaded(&speech_descriptor).await?;

        let language_code = decode_language_code(&language_settings);
        let detect_language = language_code.is_none();
        let transcription = self
            .models
            .transcribe(
                &speech_descriptor,
                &trimmed_audio,
                language_code.as_deref(),
                detect_language,
            )
            .await?;
        let raw = transcription.text.trim().to_string();
        if raw.is_empty() {
            return Err(anyhow::anyhow!("Transcription was empty."));
        }

        let language = if detect_language {
            detected_language_metadata(transcription.detected_language_code.as_deref())
        } else {
            selected_language_metadata(&language_settings)
        };
        let cleaned = self.cleanup_transcript(&raw, &language).await?;

        Ok(RecordingResult {
            raw,
            cleaned,
            audio_duration_seconds,
        })
    }

    async fn cleanup_transcript(
        &self,
        raw: &str,
        language: &DictationLanguageMetadata,
    ) -> anyhow::Result<String> {
        if !self.settings.cleanup_enabled {
            return Ok(raw.trim().to_string());
        }

        let cleanup_id = if self.settings.cleanup_model_id.trim().is_empty() {
            DEFAULT_CLEANUP_MODEL_ID
        } else {
            self.settings.cleanup_model_id.as_str()
        };
        let Some(descriptor) = cleanup_model_for(cleanup_id)
            .filter(|model| model.platforms.contains(&self.platform))
            .filter(|model| model.architectures.contains(&self.architecture))
        else {
            return Ok(raw.trim().to_string());
        };

        let state = self.models.model_state(&descriptor).await?;
        if state.local_bytes <= 0 || state.error.is_some() {
            return Ok(raw.trim().to_string());
        }

        let cleanup_rules = if self.settings.cleanup_prompt.trim().is_empty() {
            self.cleanup_default_instructions.clone()
        } else {
            self.settings.cleanup_prompt.clone()
        };
        let prompt = assemble_cleanup_prompt(&CleanupPromptInput {
            cleanup_rules,
            dictionary_entries: self.settings.dictionary_entries.clone(),
            raw_transcript: raw.to_string(),
            language: language.clone(),
            prompt_format: descriptor
                .prompt_format
                .ok_or_else(|| anyhow::anyhow!("cleanup model is missing prompt format"))?,
            default_output_tokens: descriptor.output_tokens.unwrap_or(512),
        });

        match self.models.cleanup(&descriptor, &prompt).await {
            Ok(output) => {
                let sanitized = parrot_cleanup::sanitize(&output);
                if sanitized.trim().is_empty() {
                    return Err(anyhow::anyhow!("Cleanup model produced an empty response."));
                }
                Ok(sanitized)
            }
            Err(_error) if !self.debug_cleanup_failures => Ok(raw.trim().to_string()),
            Err(error) => Err(error),
        }
    }

    async fn paste_recording_result(
        &self,
        result: &RecordingResult,
        target: Option<&PasteTarget>,
    ) -> Option<String> {
        if result.cleaned.trim().is_empty() {
            return None;
        }

        let context = self
            .adapter
            .focused_text_before_cursor(target)
            .await
            .ok()
            .flatten();
        let formatted = parrot_paste::format_contextual_paste(&result.cleaned, context.as_deref());
        self.adapter
            .paste_text(&formatted, target)
            .await
            .err()
            .map(|error| error.to_string())
    }

    fn should_capture_hotkey_paste_target(&self) -> bool {
        self.settings.paste_into_recording_start_window || matches!(self.platform, Platform::Linux)
    }

    async fn require_downloaded(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
        let state = self.models.model_state(descriptor).await?;
        if state.downloading {
            return Err(anyhow::anyhow!(
                "{} is still downloading.",
                descriptor.display_name
            ));
        }
        if state.local_bytes <= 0 || state.error.is_some() {
            return Err(anyhow::anyhow!(
                "{} download required.",
                descriptor.display_name
            ));
        }
        Ok(())
    }

    fn catalog_descriptors(&self) -> Vec<ModelDescriptor> {
        catalog()
            .models
            .into_iter()
            .filter(|model| model.platforms.contains(&self.platform))
            .filter(|model| model.architectures.contains(&self.architecture))
            .collect()
    }

    fn descriptor_for_public_id(&self, public_id: &str) -> Option<ModelDescriptor> {
        self.catalog_descriptors()
            .into_iter()
            .find(|model| model.public_id == public_id)
    }

    fn required_descriptors(&self) -> Vec<ModelDescriptor> {
        required_models(
            &DictationLanguageSettings::from(&self.settings),
            self.platform,
            self.architecture,
        )
    }

    pub fn hotkey_monitor_failed(message: impl Into<String>) -> NativeCoreEvent {
        event(
            NATIVE_CORE_EVENT_HOTKEY_MONITOR_FAILED,
            json!({ "error": message.into() }),
        )
    }
}

fn event(name: &str, payload: serde_json::Value) -> NativeCoreEvent {
    NativeCoreEvent {
        event: name.to_string(),
        payload,
    }
}

fn duration_seconds(audio: &RecordedAudio) -> f64 {
    let sample_rate = audio.sample_rate_hz.max(1) as f64;
    let channels = audio.channels.max(1) as f64;
    audio.samples.len() as f64 / sample_rate / channels
}

fn settings_platform(platform: Platform) -> SettingsPlatform {
    match platform {
        Platform::Macos => SettingsPlatform::Macos,
        Platform::Windows => SettingsPlatform::Windows,
        Platform::Linux => SettingsPlatform::Linux,
    }
}

pub const MACOS_PLATFORM_ADAPTER_FILES: &[(&str, &str)] = &[
    ("AudioRecorder.swift", "macOS audio adapter"),
    ("HotkeyMonitor.swift", "macOS hotkey adapter"),
    ("PermissionManager.swift", "macOS permission adapter"),
    (
        "FocusedTextContextReader.swift",
        "macOS focused text adapter",
    ),
    ("TextPaster.swift", "macOS paste adapter"),
    ("InputDeviceManager.swift", "macOS input device adapter"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::PermissionState;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone)]
    struct FakeAdapter {
        state: Arc<Mutex<FakeAdapterState>>,
    }

    struct FakeAdapterState {
        audio: RecordedAudio,
        started: bool,
        stopped_count: usize,
        paste_target: Option<PasteTarget>,
        last_paste_target: Option<PasteTarget>,
        focused_context: Option<String>,
        pasted_text: Option<String>,
        paste_error: Option<String>,
        sounds: Vec<SoundEvent>,
    }

    impl Default for FakeAdapterState {
        fn default() -> Self {
            Self {
                audio: RecordedAudio {
                    samples: Vec::new(),
                    sample_rate_hz: 16_000,
                    channels: 1,
                },
                started: false,
                stopped_count: 0,
                paste_target: None,
                last_paste_target: None,
                focused_context: None,
                pasted_text: None,
                paste_error: None,
                sounds: Vec::new(),
            }
        }
    }

    impl FakeAdapter {
        fn new(audio: RecordedAudio) -> Self {
            Self {
                state: Arc::new(Mutex::new(FakeAdapterState {
                    audio,
                    paste_target: Some(PasteTarget {
                        platform_id: "window-1".into(),
                    }),
                    ..FakeAdapterState::default()
                })),
            }
        }
    }

    #[async_trait]
    impl PlatformAdapter for FakeAdapter {
        async fn list_audio_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
            Ok(vec![AudioDevice {
                uid: "default".into(),
                name: "Default Microphone".into(),
                is_default: true,
            }])
        }

        async fn start_audio_recording(&self, _input_uid: Option<&str>) -> anyhow::Result<()> {
            let mut state = self.state.lock().unwrap();
            if state.started {
                return Err(anyhow::anyhow!("audio recording is already active"));
            }
            state.started = true;
            Ok(())
        }

        async fn stop_audio_recording(&self) -> anyhow::Result<RecordedAudio> {
            let mut state = self.state.lock().unwrap();
            if !state.started {
                return Err(anyhow::anyhow!("audio recording is not active"));
            }
            state.started = false;
            state.stopped_count += 1;
            Ok(state.audio.clone())
        }

        async fn permission_snapshot(&self) -> anyhow::Result<PermissionSnapshot> {
            Ok(PermissionSnapshot {
                all_required_granted: true,
                microphone: Some(PermissionState::Granted),
                ..PermissionSnapshot::default()
            })
        }

        async fn request_permission(
            &self,
            _kind: PermissionKind,
            _open_settings: bool,
        ) -> anyhow::Result<PermissionSnapshot> {
            self.permission_snapshot().await
        }

        async fn start_hotkey_monitor(&self, _shortcuts: ShortcutBindings) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop_hotkey_monitor(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn capture_shortcut(
            &self,
            _target: ShortcutTarget,
        ) -> anyhow::Result<ShortcutSettings> {
            Ok(parrot_protocol::default_windows_push_to_talk_shortcut())
        }

        async fn capture_paste_target(&self) -> anyhow::Result<Option<PasteTarget>> {
            Ok(self.state.lock().unwrap().paste_target.clone())
        }

        async fn focused_text_before_cursor(
            &self,
            _target: Option<&PasteTarget>,
        ) -> anyhow::Result<Option<String>> {
            Ok(self.state.lock().unwrap().focused_context.clone())
        }

        async fn paste_text(&self, text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()> {
            let mut state = self.state.lock().unwrap();
            if let Some(error) = state.paste_error.clone() {
                return Err(anyhow::anyhow!(error));
            }
            state.last_paste_target = target.cloned();
            state.pasted_text = Some(text.to_string());
            Ok(())
        }

        fn play_sound(&self, event: SoundEvent, _enabled: bool) {
            self.state.lock().unwrap().sounds.push(event);
        }
    }

    #[derive(Clone)]
    struct FakePipeline {
        state: Arc<Mutex<FakePipelineState>>,
    }

    struct FakePipelineState {
        states: HashMap<String, ModelFileState>,
        transcript: TranscriptionOutput,
        cleanup_output: String,
        warm_calls: Vec<String>,
        cleanup_prompt: Option<String>,
    }

    impl FakePipeline {
        fn downloaded() -> Self {
            let mut states = HashMap::new();
            states.insert("speech".into(), downloaded_state());
            states.insert("cleanup".into(), downloaded_state());
            Self {
                state: Arc::new(Mutex::new(FakePipelineState {
                    states,
                    transcript: TranscriptionOutput {
                        text: "hello world".into(),
                        detected_language_code: Some("en".into()),
                    },
                    cleanup_output: "Cleaned text: Hello, world.".into(),
                    warm_calls: Vec::new(),
                    cleanup_prompt: None,
                })),
            }
        }
    }

    #[async_trait]
    impl ModelPipeline for FakePipeline {
        async fn model_state(
            &self,
            descriptor: &ModelDescriptor,
        ) -> anyhow::Result<ModelFileState> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .states
                .get(&descriptor.public_id)
                .cloned()
                .unwrap_or_else(missing_state))
        }

        async fn download_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
            self.state
                .lock()
                .unwrap()
                .states
                .insert(descriptor.public_id.clone(), downloaded_state());
            Ok(())
        }

        async fn delete_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
            self.state
                .lock()
                .unwrap()
                .states
                .remove(&descriptor.public_id);
            Ok(())
        }

        async fn warm_speech_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
            self.state
                .lock()
                .unwrap()
                .warm_calls
                .push(descriptor.public_id.clone());
            Ok(())
        }

        async fn warm_cleanup_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
            self.state
                .lock()
                .unwrap()
                .warm_calls
                .push(descriptor.public_id.clone());
            Ok(())
        }

        async fn transcribe(
            &self,
            _descriptor: &ModelDescriptor,
            _audio: &RecordedAudio,
            _language_code: Option<&str>,
            _detect_language: bool,
        ) -> anyhow::Result<TranscriptionOutput> {
            Ok(self.state.lock().unwrap().transcript.clone())
        }

        async fn cleanup(
            &self,
            _descriptor: &ModelDescriptor,
            prompt: &parrot_prompts::CleanupPrompt,
        ) -> anyhow::Result<String> {
            let mut state = self.state.lock().unwrap();
            state.cleanup_prompt = Some(prompt.full_prompt.clone());
            Ok(state.cleanup_output.clone())
        }
    }

    fn downloaded_state() -> ModelFileState {
        ModelFileState {
            local_bytes: 100,
            downloading: false,
            progress_bytes: 100,
            progress_total_bytes: 100,
            error: None,
        }
    }

    fn missing_state() -> ModelFileState {
        ModelFileState {
            local_bytes: 0,
            downloading: false,
            progress_bytes: 0,
            progress_total_bytes: 1,
            error: None,
        }
    }

    fn speech_audio() -> RecordedAudio {
        let mut samples = vec![0.0; 2_000];
        samples.extend(vec![0.1; 8_000]);
        samples.extend(vec![0.0; 2_000]);

        RecordedAudio {
            samples,
            sample_rate_hz: 16_000,
            channels: 1,
        }
    }

    fn service(
        adapter: FakeAdapter,
        models: FakePipeline,
    ) -> CoreService<FakeAdapter, FakePipeline> {
        service_for_platform(adapter, models, Platform::Macos)
    }

    fn service_for_platform(
        adapter: FakeAdapter,
        models: FakePipeline,
        platform: Platform,
    ) -> CoreService<FakeAdapter, FakePipeline> {
        CoreService::new(
            adapter,
            models,
            CoreServiceConfig {
                settings: AppSettings::default(),
                platform,
                architecture: Architecture::Intel,
                cleanup_default_instructions: "Clean it.".into(),
                debug_cleanup_failures: false,
            },
        )
    }

    #[tokio::test]
    async fn shared_service_covers_normal_recording() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        let service = service(adapter.clone(), models.clone());

        service.start_recording().await.unwrap();
        let result = service.stop_recording().await.unwrap();

        assert_eq!(result.raw, "hello world");
        assert_eq!(result.cleaned, "Hello, world.");
        assert_eq!(
            adapter.state.lock().unwrap().sounds,
            vec![SoundEvent::RecordingStart, SoundEvent::RecordingSuccess]
        );
        assert!(models
            .state
            .lock()
            .unwrap()
            .cleanup_prompt
            .as_ref()
            .unwrap()
            .contains("hello world"));
    }

    #[tokio::test]
    async fn shared_service_covers_test_recording_with_cleanup_disabled() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        let mut service = service(adapter, models);
        let mut settings = AppSettings::default();
        settings.cleanup_enabled = false;
        service.update_settings(settings);

        service.start_recording().await.unwrap();
        let result = service.stop_recording().await.unwrap();

        assert_eq!(result.raw, "hello world");
        assert_eq!(result.cleaned, "hello world");
    }

    #[tokio::test]
    async fn shared_service_covers_hotkey_recording_and_contextual_paste() {
        let adapter = FakeAdapter::new(speech_audio());
        adapter.state.lock().unwrap().focused_context = Some("Well.".into());
        let models = FakePipeline::downloaded();
        let mut service = service(adapter.clone(), models);
        let mut settings = AppSettings::default();
        settings.paste_into_recording_start_window = true;
        service.update_settings(settings);

        let started = service.start_hotkey_recording().await;
        let events = service.stop_hotkey_recording().await;

        assert_eq!(started.event, NATIVE_CORE_EVENT_RECORDING_STARTED);
        assert_eq!(events[0].event, NATIVE_CORE_EVENT_RECORDING_PROCESSING);
        assert_eq!(events[1].event, NATIVE_CORE_EVENT_RECORDING_FINISHED);
        assert_eq!(
            adapter.state.lock().unwrap().pasted_text.as_deref(),
            Some(" Hello, world.")
        );
    }

    #[tokio::test]
    async fn linux_hotkey_recording_captures_paste_target_by_default() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        let mut service = service_for_platform(adapter.clone(), models, Platform::Linux);

        let started = service.start_hotkey_recording().await;
        let events = service.stop_hotkey_recording().await;

        assert_eq!(started.event, NATIVE_CORE_EVENT_RECORDING_STARTED);
        assert_eq!(events[1].event, NATIVE_CORE_EVENT_RECORDING_FINISHED);
        assert_eq!(
            adapter.state.lock().unwrap().pasted_text.as_deref(),
            Some("Hello, world.")
        );
        assert_eq!(
            adapter.state.lock().unwrap().last_paste_target,
            Some(PasteTarget {
                platform_id: "window-1".into()
            })
        );
    }

    #[tokio::test]
    async fn duplicate_hotkey_start_keeps_active_recording_state() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        let mut service = service(adapter.clone(), models);

        let started = service.start_hotkey_recording().await;
        let duplicate = service.start_hotkey_recording().await;
        let events = service.stop_hotkey_recording().await;

        assert_eq!(started.event, NATIVE_CORE_EVENT_RECORDING_STARTED);
        assert_eq!(duplicate.event, NATIVE_CORE_EVENT_RECORDING_STARTED);
        assert_eq!(duplicate.payload["busy"], true);
        assert_eq!(events[0].event, NATIVE_CORE_EVENT_RECORDING_PROCESSING);
        assert_eq!(events[1].event, NATIVE_CORE_EVENT_RECORDING_FINISHED);
        assert_eq!(adapter.state.lock().unwrap().stopped_count, 1);
    }

    #[tokio::test]
    async fn split_hotkey_stop_marks_processing_before_transcription() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        let mut service = service(adapter.clone(), models);

        let _ = service.start_hotkey_recording().await;
        let stop = service.begin_stop_hotkey_recording();

        let paste_target = match stop {
            HotkeyStopRecording::Started {
                event,
                paste_target,
            } => {
                assert_eq!(event.event, NATIVE_CORE_EVENT_RECORDING_PROCESSING);
                paste_target
            }
            other => panic!("expected stop to start processing, got {other:?}"),
        };
        assert!(!service.hotkey_recording_active());
        assert!(service.hotkey_recording_processing());

        let finished = service.finish_stopped_hotkey_recording(paste_target).await;
        assert_eq!(finished.event, NATIVE_CORE_EVENT_RECORDING_FINISHED);
        assert!(!service.hotkey_recording_processing());
        assert_eq!(adapter.state.lock().unwrap().stopped_count, 1);
    }

    #[tokio::test]
    async fn shared_service_covers_cancellation() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        let mut service = service(adapter.clone(), models);

        let _ = service.start_hotkey_recording().await;
        let cancelled = service.cancel_hotkey_recording().await;

        assert_eq!(cancelled.event, NATIVE_CORE_EVENT_RECORDING_CANCELLED);
        assert_eq!(adapter.state.lock().unwrap().stopped_count, 1);
    }

    #[tokio::test]
    async fn shared_service_covers_paste_failure_payload() {
        let adapter = FakeAdapter::new(speech_audio());
        adapter.state.lock().unwrap().paste_error = Some("paste denied".into());
        let models = FakePipeline::downloaded();
        let mut service = service(adapter, models);

        let _ = service.start_hotkey_recording().await;
        let events = service.stop_hotkey_recording().await;

        assert_eq!(events[1].event, NATIVE_CORE_EVENT_RECORDING_FINISHED);
        assert_eq!(events[1].payload["pasteError"], "paste denied");
    }

    #[tokio::test]
    async fn shared_service_covers_model_missing_failure() {
        let adapter = FakeAdapter::new(speech_audio());
        let models = FakePipeline::downloaded();
        models.state.lock().unwrap().states.insert(
            "speech".into(),
            ModelFileState {
                local_bytes: 0,
                downloading: false,
                progress_bytes: 0,
                progress_total_bytes: 1,
                error: None,
            },
        );
        let service = service(adapter, models);

        service.start_recording().await.unwrap();
        let error = service.stop_recording().await.unwrap_err();

        assert!(error.to_string().contains("download required"));
    }

    #[test]
    fn documents_current_macos_adapter_files() {
        let files = MACOS_PLATFORM_ADAPTER_FILES
            .iter()
            .map(|(file, _)| *file)
            .collect::<Vec<_>>();

        assert!(files.contains(&"AudioRecorder.swift"));
        assert!(files.contains(&"HotkeyMonitor.swift"));
        assert!(files.contains(&"TextPaster.swift"));
    }
}
