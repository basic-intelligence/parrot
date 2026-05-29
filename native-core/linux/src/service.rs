use crate::{
    json_lines::{error_response, event_message, success_response, RequestLine},
    model_downloads::LinuxModelStore,
    model_llama_cpp::LlamaCleanupPipeline,
    model_whisper_cpp::WhisperCppPipeline,
    platform::{
        audio::AudioManager,
        focused_text,
        hotkeys::{HotkeyAction, HotkeyMonitor, HotkeySource},
        paste::{self, PasteTarget as LinuxPasteTarget},
        permissions::PermissionManager,
        shortcut_capture::{self, ShortcutCaptureTarget},
        sound,
    },
};
use async_trait::async_trait;
use parrot_audio::RecordedAudio;
use parrot_core_service::{
    CoreService, CoreServiceConfig, HotkeyStopRecording, ModelPipeline, PasteTarget,
    PlatformAdapter, ShortcutBindings, ShortcutTarget, TranscriptionOutput,
};
use parrot_models::{Architecture, ModelDescriptor, ModelFileState, Platform};
use parrot_prompts::CleanupPrompt;
use parrot_protocol::{
    AppSettings, AudioDevice, ModelRole, NativeCoreEvent, NativeCoreMethod, NativeCorePaths,
    PermissionKind, PermissionSnapshot, ShortcutSettings, SoundEvent,
    NATIVE_CORE_EVENT_RECORDING_FAILED,
};
use parrot_settings::{default_settings_for_platform, SettingsPlatform};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

type SharedCore = CoreService<LinuxPlatformAdapter, LinuxModelPipeline>;

pub struct LinuxNativeService {
    core: Arc<tokio::sync::Mutex<SharedCore>>,
    adapter: LinuxPlatformAdapter,
    models: LinuxModelPipeline,
    settings: AppSettings,
    cleanup_default_instructions: String,
    debug_cleanup_failures: bool,
    output_tx: mpsc::UnboundedSender<Value>,
}

impl LinuxNativeService {
    pub fn new(output_tx: mpsc::UnboundedSender<Value>) -> Self {
        let settings = default_settings_for_platform(SettingsPlatform::Linux);
        let (action_tx, action_rx) = mpsc::unbounded_channel();
        let adapter = LinuxPlatformAdapter::new(action_tx);
        let models = LinuxModelPipeline::default();
        let core = Arc::new(tokio::sync::Mutex::new(core_service(
            adapter.clone(),
            models.clone(),
            settings.clone(),
            String::new(),
            false,
        )));
        spawn_hotkey_worker(
            core.clone(),
            adapter.hotkeys.clone(),
            action_rx,
            output_tx.clone(),
        );

        Self {
            core,
            adapter,
            models,
            settings,
            cleanup_default_instructions: String::new(),
            debug_cleanup_failures: false,
            output_tx,
        }
    }

    pub async fn handle_request(&mut self, request: RequestLine) -> Value {
        let result: anyhow::Result<Value> = match request.method.as_str() {
            method if method == NativeCoreMethod::Initialize.as_str() => {
                self.initialize(request.payload).await
            }
            method if method == NativeCoreMethod::PermissionStatuses.as_str() => {
                self.permission_statuses().await
            }
            method if method == NativeCoreMethod::RequestPermission.as_str() => {
                self.request_permission(request.payload).await
            }
            method if method == NativeCoreMethod::WarmModels.as_str() => self.warm_models().await,
            method if method == NativeCoreMethod::UpdateSettings.as_str() => {
                self.update_settings(request.payload).await
            }
            method if method == NativeCoreMethod::ModelStatuses.as_str() => {
                self.model_statuses().await
            }
            method if method == NativeCoreMethod::DownloadModel.as_str() => {
                self.download_model(request.payload).await
            }
            method if method == NativeCoreMethod::DeleteModel.as_str() => {
                self.delete_model(request.payload).await
            }
            method if method == NativeCoreMethod::ListAudioDevices.as_str() => {
                self.list_audio_devices().await
            }
            method if method == NativeCoreMethod::StartRecording.as_str() => {
                self.start_recording().await
            }
            method if method == NativeCoreMethod::StopRecording.as_str() => {
                self.stop_recording().await
            }
            method if method == NativeCoreMethod::StartHotkeyMonitor.as_str() => {
                self.start_hotkey_monitor().await
            }
            method if method == NativeCoreMethod::StopHotkeyMonitor.as_str() => {
                self.stop_hotkey_monitor().await
            }
            method if method == NativeCoreMethod::CaptureShortcut.as_str() => {
                self.capture_shortcut(request.payload).await
            }
            method if method == NativeCoreMethod::StartHotkeyRecording.as_str() => {
                self.start_hotkey_recording().await
            }
            method if method == NativeCoreMethod::StopHotkeyRecording.as_str() => {
                self.stop_hotkey_recording().await
            }
            method if method == NativeCoreMethod::ToggleHotkeyRecording.as_str() => {
                self.toggle_hotkey_recording().await
            }
            method if method == NativeCoreMethod::CancelHotkeyRecording.as_str() => {
                self.cancel_hotkey_recording().await
            }
            method => Err(anyhow::anyhow!("Unknown native-core method: {method}")),
        };

        match result {
            Ok(payload) => success_response(&request.id, payload),
            Err(error) => error_response(&request.id, error.to_string()),
        }
    }

    async fn initialize(&mut self, payload: Value) -> anyhow::Result<Value> {
        if let Some(settings_value) = payload.get("settings") {
            self.settings = serde_json::from_value(settings_value.clone())?;
        }

        if let Some(paths_value) = payload.get("paths") {
            let paths: NativeCorePaths = serde_json::from_value(paths_value.clone())?;
            self.adapter.configure_paths(paths.clone());
            self.models.configure_paths(paths);
        }

        self.cleanup_default_instructions = payload
            .get("prompts")
            .and_then(|prompts| prompts.get("cleanupDefaultInstructions"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.debug_cleanup_failures = payload
            .get("debugCleanupFailures")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        *self.core.lock().await = core_service(
            self.adapter.clone(),
            self.models.clone(),
            self.settings.clone(),
            self.cleanup_default_instructions.clone(),
            self.debug_cleanup_failures,
        );

        Ok(json!({ "status": "initialized" }))
    }

    async fn update_settings(&mut self, payload: Value) -> anyhow::Result<Value> {
        let Some(settings_value) = payload.get("settings") else {
            return Err(anyhow::anyhow!(
                "updateSettings payload is missing `settings`"
            ));
        };

        let settings: AppSettings = serde_json::from_value(settings_value.clone())?;
        let updated = self.core.lock().await.update_settings(settings);
        self.settings = updated.clone();
        Ok(serde_json::to_value(updated)?)
    }

    async fn permission_statuses(&self) -> anyhow::Result<Value> {
        Ok(serde_json::to_value(
            self.core.lock().await.permission_snapshot().await?,
        )?)
    }

    async fn request_permission(&self, payload: Value) -> anyhow::Result<Value> {
        let kind_value = payload
            .get("kind")
            .ok_or_else(|| anyhow::anyhow!("requestPermission payload is missing `kind`"))?;
        let kind: PermissionKind = serde_json::from_value(kind_value.clone())?;
        let open_settings = payload
            .get("openSettings")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(serde_json::to_value(
            self.core
                .lock()
                .await
                .request_permission(kind, open_settings)
                .await?,
        )?)
    }

    async fn model_statuses(&self) -> anyhow::Result<Value> {
        Ok(serde_json::to_value(
            self.core.lock().await.model_statuses().await?,
        )?)
    }

    async fn download_model(&self, payload: Value) -> anyhow::Result<Value> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("downloadModel payload is missing `kind`"))?;
        Ok(serde_json::to_value(
            self.core.lock().await.download_model(kind).await?,
        )?)
    }

    async fn delete_model(&self, payload: Value) -> anyhow::Result<Value> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("deleteModel payload is missing `kind`"))?;
        Ok(serde_json::to_value(
            self.core.lock().await.delete_model(kind).await?,
        )?)
    }

    async fn warm_models(&self) -> anyhow::Result<Value> {
        self.core.lock().await.warm_models().await?;
        Ok(json!({ "status": "warmed" }))
    }

    async fn list_audio_devices(&self) -> anyhow::Result<Value> {
        Ok(serde_json::to_value(
            self.core.lock().await.list_audio_devices().await?,
        )?)
    }

    async fn start_recording(&self) -> anyhow::Result<Value> {
        self.core.lock().await.start_recording().await?;
        Ok(json!({ "status": "recording" }))
    }

    async fn stop_recording(&self) -> anyhow::Result<Value> {
        Ok(serde_json::to_value(
            self.core.lock().await.stop_recording().await?,
        )?)
    }

    async fn start_hotkey_monitor(&self) -> anyhow::Result<Value> {
        match self.core.lock().await.start_hotkey_monitor().await {
            Ok(()) => Ok(json!({ "status": "hotkey-monitoring" })),
            Err(error) => {
                self.emit_event(
                    CoreService::<LinuxPlatformAdapter, LinuxModelPipeline>::hotkey_monitor_failed(
                        error.to_string(),
                    ),
                );
                Err(error)
            }
        }
    }

    async fn stop_hotkey_monitor(&self) -> anyhow::Result<Value> {
        self.core.lock().await.stop_hotkey_monitor().await?;
        Ok(json!({ "status": "hotkey-stopped" }))
    }

    async fn capture_shortcut(&self, payload: Value) -> anyhow::Result<Value> {
        let target_value = payload
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("captureShortcut payload is missing `target`"))?;
        let target = match ShortcutCaptureTarget::try_from(target_value)? {
            ShortcutCaptureTarget::PushToTalk => ShortcutTarget::PushToTalk,
            ShortcutCaptureTarget::HandsFree => ShortcutTarget::HandsFree,
        };
        Ok(serde_json::to_value(
            self.core.lock().await.capture_shortcut(target).await?,
        )?)
    }

    async fn start_hotkey_recording(&self) -> anyhow::Result<Value> {
        let event = {
            let mut core = self.core.lock().await;
            tag_hotkey_source(core.start_hotkey_recording().await, HotkeySource::HandsFree)
        };

        self.emit_event(event);

        Ok(json!({ "status": "hotkey-recording-started" }))
    }

    async fn stop_hotkey_recording(&self) -> anyhow::Result<Value> {
        let events = {
            let mut core = self.core.lock().await;
            match core.begin_stop_hotkey_recording() {
                HotkeyStopRecording::Idle => Vec::new(),
                HotkeyStopRecording::Busy(event) => {
                    vec![tag_hotkey_source(event, HotkeySource::HandsFree)]
                }
                HotkeyStopRecording::Started {
                    event,
                    paste_target,
                } => {
                    self.emit_event(tag_hotkey_source(event, HotkeySource::HandsFree));
                    vec![tag_hotkey_source(
                        core.finish_stopped_hotkey_recording(paste_target).await,
                        HotkeySource::HandsFree,
                    )]
                }
            }
        };

        for event in events {
            self.emit_event(event);
        }

        Ok(json!({ "status": "hotkey-recording-stopped" }))
    }

    async fn toggle_hotkey_recording(&self) -> anyhow::Result<Value> {
        let active = self.core.lock().await.hotkey_recording_active();

        if active {
            self.stop_hotkey_recording().await
        } else {
            self.start_hotkey_recording().await
        }
    }

    async fn cancel_hotkey_recording(&self) -> anyhow::Result<Value> {
        let event = {
            let mut core = self.core.lock().await;
            core.cancel_hotkey_recording().await
        };

        self.emit_event(event);

        Ok(json!({ "status": "hotkey-recording-cancelled" }))
    }

    fn emit_event(&self, event: NativeCoreEvent) {
        let _ = self
            .output_tx
            .send(event_message(&event.event, event.payload));
    }
}

fn core_service(
    adapter: LinuxPlatformAdapter,
    models: LinuxModelPipeline,
    settings: AppSettings,
    cleanup_default_instructions: String,
    debug_cleanup_failures: bool,
) -> SharedCore {
    CoreService::new(
        adapter,
        models,
        CoreServiceConfig {
            settings,
            platform: Platform::Linux,
            architecture: Architecture::X86_64,
            cleanup_default_instructions,
            debug_cleanup_failures,
        },
    )
}

fn spawn_hotkey_worker(
    core: Arc<tokio::sync::Mutex<SharedCore>>,
    hotkeys: HotkeyMonitor,
    mut action_rx: mpsc::UnboundedReceiver<HotkeyAction>,
    output_tx: mpsc::UnboundedSender<Value>,
) {
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            let events = {
                let mut core = core.lock().await;
                match action {
                    HotkeyAction::Start { source } => {
                        let event = tag_hotkey_source(core.start_hotkey_recording().await, source);
                        if event.event == NATIVE_CORE_EVENT_RECORDING_FAILED {
                            hotkeys.force_toggle_off(source);
                            hotkeys.set_cancellation_enabled(false);
                        } else {
                            hotkeys.set_cancellation_enabled(true);
                        }
                        vec![event]
                    }
                    HotkeyAction::Stop { source } => {
                        let events = match core.begin_stop_hotkey_recording() {
                            HotkeyStopRecording::Idle => Vec::new(),
                            HotkeyStopRecording::Busy(event) => {
                                vec![tag_hotkey_source(event, source)]
                            }
                            HotkeyStopRecording::Started {
                                event,
                                paste_target,
                            } => {
                                let event = tag_hotkey_source(event, source);
                                let event_name = event.event.clone();
                                let _ = output_tx.send(event_message(&event_name, event.payload));
                                vec![tag_hotkey_source(
                                    core.finish_stopped_hotkey_recording(paste_target).await,
                                    source,
                                )]
                            }
                        };
                        hotkeys.set_cancellation_enabled(false);
                        events
                    }
                    HotkeyAction::Cancel => {
                        hotkeys.set_cancellation_enabled(false);
                        vec![core.cancel_hotkey_recording().await]
                    }
                }
            };

            for event in events {
                let _ = output_tx.send(event_message(&event.event, event.payload));
            }
        }
    });
}

fn tag_hotkey_source(mut event: NativeCoreEvent, source: HotkeySource) -> NativeCoreEvent {
    if let Some(payload) = event.payload.as_object_mut() {
        payload.insert("source".into(), json!(source.as_str()));
    }
    event
}

#[derive(Clone)]
pub struct LinuxPlatformAdapter {
    audio: AudioManager,
    permissions: Arc<PermissionManager>,
    hotkeys: HotkeyMonitor,
    hotkey_action_tx: mpsc::UnboundedSender<HotkeyAction>,
    paths: Arc<Mutex<Option<NativeCorePaths>>>,
}

impl LinuxPlatformAdapter {
    fn new(hotkey_action_tx: mpsc::UnboundedSender<HotkeyAction>) -> Self {
        Self {
            audio: AudioManager::default(),
            permissions: Arc::new(PermissionManager),
            hotkeys: HotkeyMonitor::default(),
            hotkey_action_tx,
            paths: Arc::new(Mutex::new(None)),
        }
    }

    fn configure_paths(&self, paths: NativeCorePaths) {
        *self.paths.lock().expect("platform paths poisoned") = Some(paths);
    }
}

#[async_trait]
impl PlatformAdapter for LinuxPlatformAdapter {
    async fn list_audio_devices(&self) -> anyhow::Result<Vec<AudioDevice>> {
        self.audio.list_input_devices()
    }

    async fn start_audio_recording(&self, input_uid: Option<&str>) -> anyhow::Result<()> {
        self.audio.start_recording(input_uid)
    }

    async fn stop_audio_recording(&self) -> anyhow::Result<RecordedAudio> {
        self.audio.stop_recording()
    }

    async fn permission_snapshot(&self) -> anyhow::Result<PermissionSnapshot> {
        Ok(self.permissions.statuses())
    }

    async fn request_permission(
        &self,
        kind: PermissionKind,
        open_settings: bool,
    ) -> anyhow::Result<PermissionSnapshot> {
        self.permissions.request_permission(kind, open_settings)
    }

    async fn start_hotkey_monitor(&self, shortcuts: ShortcutBindings) -> anyhow::Result<()> {
        self.hotkeys.start(
            shortcuts.push_to_talk,
            shortcuts.hands_free,
            self.hotkey_action_tx.clone(),
        )
    }

    async fn stop_hotkey_monitor(&self) -> anyhow::Result<()> {
        self.hotkeys.stop();
        Ok(())
    }

    async fn capture_shortcut(&self, target: ShortcutTarget) -> anyhow::Result<ShortcutSettings> {
        let target = match target {
            ShortcutTarget::PushToTalk => ShortcutCaptureTarget::PushToTalk,
            ShortcutTarget::HandsFree => ShortcutCaptureTarget::HandsFree,
        };
        shortcut_capture::capture(target)
    }

    async fn capture_paste_target(&self) -> anyhow::Result<Option<PasteTarget>> {
        Ok(paste::capture_current_target().map(|target| PasteTarget {
            platform_id: target.platform_id(),
        }))
    }

    async fn focused_text_before_cursor(
        &self,
        target: Option<&PasteTarget>,
    ) -> anyhow::Result<Option<String>> {
        let linux_target =
            target.map(|target| LinuxPasteTarget::from_platform_id(target.platform_id.clone()));
        Ok(focused_text::text_before_cursor(linux_target.as_ref()))
    }

    async fn paste_text(&self, text: &str, target: Option<&PasteTarget>) -> anyhow::Result<()> {
        let linux_target =
            target.map(|target| LinuxPasteTarget::from_platform_id(target.platform_id.clone()));
        paste::paste_text(text, linux_target.as_ref())
    }

    fn play_sound(&self, event: SoundEvent, enabled: bool) {
        let paths = self.paths.lock().expect("platform paths poisoned").clone();
        sound::play(event, enabled, paths.as_ref());
    }
}

#[derive(Clone, Default)]
pub struct LinuxModelPipeline {
    store: LinuxModelStore,
    speech: WhisperCppPipeline,
    cleanup: LlamaCleanupPipeline,
}

impl LinuxModelPipeline {
    fn configure_paths(&self, paths: NativeCorePaths) {
        self.store.configure_paths(paths);
    }
}

#[async_trait]
impl ModelPipeline for LinuxModelPipeline {
    async fn model_state(&self, descriptor: &ModelDescriptor) -> anyhow::Result<ModelFileState> {
        self.store.state(descriptor)
    }

    async fn download_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
        self.store.start_download(descriptor)
    }

    async fn delete_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
        self.store.delete_model(descriptor)?;
        if descriptor.role == ModelRole::Cleanup {
            self.cleanup.clear_cache();
        }
        Ok(())
    }

    async fn warm_speech_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
        self.speech
            .warm_descriptor(descriptor, &self.store.paths()?)
    }

    async fn warm_cleanup_model(&self, descriptor: &ModelDescriptor) -> anyhow::Result<()> {
        self.cleanup
            .warm_descriptor(descriptor, &self.store.paths()?)
    }

    async fn transcribe(
        &self,
        descriptor: &ModelDescriptor,
        audio: &RecordedAudio,
        language_code: Option<&str>,
        detect_language: bool,
    ) -> anyhow::Result<TranscriptionOutput> {
        self.speech.transcribe_descriptor(
            descriptor,
            audio,
            &self.store.paths()?,
            language_code,
            detect_language,
        )
    }

    async fn cleanup(
        &self,
        descriptor: &ModelDescriptor,
        prompt: &CleanupPrompt,
    ) -> anyhow::Result<String> {
        self.cleanup
            .cleanup_descriptor(descriptor, &self.store.paths()?, prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::DictationLanguageMode;
    use tempfile::TempDir;

    fn temp_paths(temp: &TempDir) -> NativeCorePaths {
        NativeCorePaths {
            app_data_dir: temp.path().join("app-data").display().to_string(),
            models_dir: temp.path().join("models").display().to_string(),
            speech_models_dir: temp.path().join("models/speech").display().to_string(),
            cleanup_models_dir: temp.path().join("models/cleanup").display().to_string(),
            resources_dir: temp.path().join("resources").display().to_string(),
            shared_resources_dir: temp.path().join("resources/shared").display().to_string(),
            temp_dir: temp.path().join("temp").display().to_string(),
        }
    }

    #[tokio::test]
    async fn initialize_stores_settings_and_paths() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut service = LinuxNativeService::new(tx);
        let temp = TempDir::new().unwrap();
        let paths = temp_paths(&temp);

        let response = service
            .handle_request(RequestLine {
                id: "init".into(),
                method: "initialize".into(),
                payload: json!({
                    "settings": AppSettings {
                        dictation_language_mode: DictationLanguageMode::Detect,
                        ..AppSettings::default()
                    },
                    "paths": paths,
                    "prompts": {
                        "cleanupDefaultInstructions": "Clean this transcript."
                    },
                    "debugCleanupFailures": true
                }),
            })
            .await;

        assert_eq!(response["ok"], true);
        assert_eq!(response["payload"]["status"], "initialized");
        assert_eq!(
            service.settings.dictation_language_mode,
            DictationLanguageMode::Detect
        );
        assert_eq!(
            service.cleanup_default_instructions,
            "Clean this transcript."
        );
        assert!(service.debug_cleanup_failures);
    }

    #[tokio::test]
    async fn unknown_methods_return_structured_errors() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut service = LinuxNativeService::new(tx);

        let response = service
            .handle_request(RequestLine {
                id: "bad".into(),
                method: "unknown".into(),
                payload: json!({}),
            })
            .await;

        assert_eq!(response["ok"], false);
        assert_eq!(response["id"], "bad");
        assert!(response["error"]
            .as_str()
            .unwrap()
            .contains("Unknown native-core method"));
    }

    #[tokio::test]
    async fn model_status_returns_linux_catalog_entries() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut service = LinuxNativeService::new(tx);
        let temp = TempDir::new().unwrap();
        service
            .handle_request(RequestLine {
                id: "init".into(),
                method: "initialize".into(),
                payload: json!({
                    "settings": AppSettings {
                        dictation_language_mode: DictationLanguageMode::Detect,
                        cleanup_model_id: "cleanup-gemma-4-e2b".into(),
                        ..AppSettings::default()
                    },
                    "paths": temp_paths(&temp),
                }),
            })
            .await;

        let response = service
            .handle_request(RequestLine {
                id: "models".into(),
                method: "modelStatuses".into(),
                payload: json!({}),
            })
            .await;

        assert_eq!(response["ok"], true);
        let models = response["payload"].as_array().unwrap();
        assert_eq!(models.len(), 4);
        assert_eq!(models[0]["id"], "speech");
        assert_eq!(models[1]["id"], "speech-multilingual");
        assert_eq!(models[1]["required"], true);
        assert_eq!(models[2]["id"], "cleanup");
        assert_eq!(models[3]["id"], "cleanup-gemma-4-e2b");
        assert_eq!(models[3]["required"], true);
    }

    #[tokio::test]
    async fn permission_status_returns_linux_permission_requirements() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut service = LinuxNativeService::new(tx);

        let response = service
            .handle_request(RequestLine {
                id: "permissions".into(),
                method: "permissionStatuses".into(),
                payload: json!({}),
            })
            .await;

        assert_eq!(response["ok"], true);
        assert_eq!(
            response["payload"]["requirements"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(response["payload"]["accessibility"], Value::Null);
        assert_eq!(response["payload"]["inputMonitoring"], Value::Null);
    }

    #[tokio::test]
    async fn hotkey_monitor_start_stop_returns_structured_statuses() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut service = LinuxNativeService::new(tx);

        let start = service
            .handle_request(RequestLine {
                id: "start".into(),
                method: "startHotkeyMonitor".into(),
                payload: json!({}),
            })
            .await;
        assert_eq!(start["ok"], true);
        assert_eq!(start["payload"]["status"], "hotkey-monitoring");

        let stop = service
            .handle_request(RequestLine {
                id: "stop".into(),
                method: "stopHotkeyMonitor".into(),
                payload: json!({}),
            })
            .await;
        assert_eq!(stop["ok"], true);
        assert_eq!(stop["payload"]["status"], "hotkey-stopped");
    }
}
