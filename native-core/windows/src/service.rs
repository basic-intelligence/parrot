#[cfg(target_os = "windows")]
use crate::platform::audio::{selected_uid_exists, RecordedAudio};
#[cfg(target_os = "windows")]
use crate::platform::paste;
use crate::{
    json_lines::{error_response, event_message, success_response, RequestLine},
    models::downloads::WindowsModelStore,
    models::llama_cpp::LlamaCleanupPipeline,
    models::whisper_cpp::WhisperCppPipeline,
    platform::audio::AudioManager,
    platform::hotkeys::{validate_shortcut_pair, HotkeyAction, HotkeyMonitor, HotkeySource},
    platform::paste::PasteTarget,
    platform::permissions::PermissionManager,
    platform::shortcut_capture::{self, ShortcutCaptureTarget},
    platform::sound::{self, SoundEvent},
};
use parrot_protocol::{
    AppSettings, NativeCoreMethod, NativeCorePaths, PermissionKind, RecordingResult,
    NATIVE_CORE_EVENT_HOTKEY_MONITOR_FAILED,
};
#[cfg(target_os = "windows")]
use parrot_protocol::{
    NATIVE_CORE_EVENT_RECORDING_CANCELLED, NATIVE_CORE_EVENT_RECORDING_FAILED,
    NATIVE_CORE_EVENT_RECORDING_FINISHED, NATIVE_CORE_EVENT_RECORDING_PROCESSING,
    NATIVE_CORE_EVENT_RECORDING_STARTED,
};
use parrot_settings::{normalize_settings_for_platform, SettingsPlatform};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

#[derive(Default)]
pub struct CoreService {
    settings: Option<AppSettings>,
    paths: Option<NativeCorePaths>,
    cleanup_prompt: String,
    debug_cleanup_failures: bool,
    models: WindowsModelStore,
    speech: WhisperCppPipeline,
    cleanup: LlamaCleanupPipeline,
    audio: AudioManager,
    permissions: PermissionManager,
    hotkeys: HotkeyMonitor,
    hotkey_runtime: HotkeyRuntime,
    event_tx: Option<mpsc::Sender<Value>>,
}

impl CoreService {
    pub fn with_event_sender(event_tx: mpsc::Sender<Value>) -> Self {
        let mut service = Self::default();
        service.event_tx = Some(event_tx);
        service
    }

    pub fn handle_request(&mut self, request: RequestLine) -> Value {
        let result: anyhow::Result<Value> = match request.method.as_str() {
            method if method == NativeCoreMethod::Initialize.as_str() => {
                self.initialize(request.payload)
            }
            method if method == NativeCoreMethod::PermissionStatuses.as_str() => {
                self.permission_statuses()
            }
            method if method == NativeCoreMethod::RequestPermission.as_str() => {
                self.request_permission(request.payload)
            }
            method if method == NativeCoreMethod::WarmModels.as_str() => self.warm_models(),
            method if method == NativeCoreMethod::UpdateSettings.as_str() => {
                self.update_settings(request.payload)
            }
            method if method == NativeCoreMethod::ModelStatuses.as_str() => self.model_statuses(),
            method if method == NativeCoreMethod::DownloadModel.as_str() => {
                self.download_model(request.payload)
            }
            method if method == NativeCoreMethod::DeleteModel.as_str() => {
                self.delete_model(request.payload)
            }
            method if method == NativeCoreMethod::ListAudioDevices.as_str() => self
                .audio
                .list_input_devices()
                .and_then(|devices| serde_json::to_value(devices).map_err(Into::into)),
            method if method == NativeCoreMethod::StartRecording.as_str() => self.start_recording(),
            method if method == NativeCoreMethod::StopRecording.as_str() => self.stop_recording(),
            method if method == NativeCoreMethod::StartHotkeyMonitor.as_str() => {
                self.start_hotkey_monitor()
            }
            method if method == NativeCoreMethod::StopHotkeyMonitor.as_str() => {
                self.stop_hotkey_monitor()
            }
            method if method == NativeCoreMethod::CaptureShortcut.as_str() => {
                self.capture_shortcut(request.payload)
            }
            method => Err(anyhow::anyhow!("Unknown native-core method: {method}")),
        };

        match result {
            Ok(payload) => success_response(&request.id, payload),
            Err(error) => error_response(&request.id, error.to_string()),
        }
    }

    fn initialize(&mut self, payload: Value) -> anyhow::Result<Value> {
        if let Some(settings_value) = payload.get("settings") {
            let mut settings: AppSettings = serde_json::from_value(settings_value.clone())?;
            self.normalize_runtime_settings(&mut settings);
            self.hotkey_runtime.set_settings(Some(settings.clone()));
            self.settings = Some(settings);
        }

        if let Some(paths_value) = payload.get("paths") {
            let paths: NativeCorePaths = serde_json::from_value(paths_value.clone())?;
            self.models.configure_paths(paths.clone());
            self.hotkey_runtime.set_paths(Some(paths.clone()));
            self.paths = Some(paths);
        }

        self.cleanup_prompt = payload
            .get("prompts")
            .and_then(|prompts| prompts.get("cleanupDefaultInstructions"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.hotkey_runtime
            .set_cleanup_prompt(self.cleanup_prompt.clone());
        self.debug_cleanup_failures = payload
            .get("debugCleanupFailures")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.hotkey_runtime
            .set_debug_cleanup_failures(self.debug_cleanup_failures);

        Ok(json!({ "status": "initialized" }))
    }

    fn update_settings(&mut self, payload: Value) -> anyhow::Result<Value> {
        let Some(settings_value) = payload.get("settings") else {
            return Err(anyhow::anyhow!(
                "updateSettings payload is missing `settings`"
            ));
        };

        let mut settings: AppSettings = serde_json::from_value(settings_value.clone())?;
        self.normalize_runtime_settings(&mut settings);
        self.hotkey_runtime.set_settings(Some(settings.clone()));
        self.settings = Some(settings.clone());
        if self.hotkeys.is_running() {
            self.hotkeys.start(
                settings.push_to_talk_shortcut.clone(),
                settings.hands_free_shortcut.clone(),
                self.hotkey_runtime
                    .worker_sender()
                    .ok_or_else(|| anyhow::anyhow!("hotkey worker is not available"))?,
            )?;
        }
        Ok(serde_json::to_value(settings)?)
    }

    fn model_statuses(&self) -> anyhow::Result<Value> {
        let settings = self.settings.clone().unwrap_or_default();
        serde_json::to_value(self.models.statuses(&settings)?).map_err(Into::into)
    }

    fn download_model(&self, payload: Value) -> anyhow::Result<Value> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("downloadModel payload is missing `kind`"))?;
        let settings = self.settings.clone().unwrap_or_default();
        serde_json::to_value(self.models.start_download(kind, &settings)?).map_err(Into::into)
    }

    fn delete_model(&self, payload: Value) -> anyhow::Result<Value> {
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("deleteModel payload is missing `kind`"))?;
        let settings = self.settings.clone().unwrap_or_default();
        serde_json::to_value(self.models.delete_model(kind, &settings)?).map_err(Into::into)
    }

    fn permission_statuses(&self) -> anyhow::Result<Value> {
        let settings = self.settings.clone().unwrap_or_default();
        serde_json::to_value(
            self.permissions
                .statuses(settings.selected_input_uid.as_deref()),
        )
        .map_err(Into::into)
    }

    fn request_permission(&self, payload: Value) -> anyhow::Result<Value> {
        let kind_value = payload
            .get("kind")
            .ok_or_else(|| anyhow::anyhow!("requestPermission payload is missing `kind`"))?;
        let kind: PermissionKind = serde_json::from_value(kind_value.clone())?;
        let open_settings = payload
            .get("openSettings")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let settings = self.settings.clone().unwrap_or_default();
        serde_json::to_value(self.permissions.request_permission(
            kind,
            open_settings,
            settings.selected_input_uid.as_deref(),
        )?)
        .map_err(Into::into)
    }

    fn warm_models(&self) -> anyhow::Result<Value> {
        let settings = self.settings.clone().unwrap_or_default();
        let Some(paths) = self.paths.clone() else {
            return Ok(json!({ "status": "not-initialized" }));
        };
        self.speech.warm(&settings, &paths)?;
        self.cleanup.warm(&settings, &paths.cleanup_models_dir)?;
        Ok(json!({ "status": "warmed" }))
    }

    fn start_recording(&self) -> anyhow::Result<Value> {
        let settings = self.settings.clone().unwrap_or_default();
        if let Err(error) = self
            .audio
            .start_recording(settings.selected_input_uid.as_deref())
        {
            sound::play(
                SoundEvent::RecordingError,
                settings.play_sounds,
                self.paths.as_ref(),
            );
            return Err(error);
        }
        sound::play(
            SoundEvent::RecordingStart,
            settings.play_sounds,
            self.paths.as_ref(),
        );
        if let Some(paths) = self.paths.clone() {
            let speech = self.speech.clone();
            std::thread::spawn(move || {
                if let Err(error) = speech.warm(&settings, &paths) {
                    eprintln!("Windows whisper.cpp warmup failed: {error}");
                }
            });
        }
        Ok(json!({ "status": "recording" }))
    }

    fn stop_recording(&self) -> anyhow::Result<Value> {
        let settings = self.settings.clone().unwrap_or_default();
        let result = (|| {
            let recorded = self.audio.stop_recording()?;
            let paths = self
                .paths
                .clone()
                .ok_or_else(|| anyhow::anyhow!("native-core paths are not initialized"))?;
            let transcription =
                self.speech
                    .transcribe(&recorded.samples_16khz, &settings, &paths)?;
            let cleaned = self.cleanup.cleanup(
                &transcription.text,
                &settings,
                &transcription.language,
                &paths.cleanup_models_dir,
                &self.cleanup_prompt,
                self.debug_cleanup_failures,
            )?;
            serde_json::to_value(RecordingResult {
                raw: transcription.text.clone(),
                cleaned,
                audio_duration_seconds: recorded.duration_seconds,
            })
            .map_err(Into::into)
        })();

        match result {
            Ok(value) => {
                sound::play(
                    SoundEvent::RecordingSuccess,
                    settings.play_sounds,
                    self.paths.as_ref(),
                );
                Ok(value)
            }
            Err(error) => {
                sound::play(
                    SoundEvent::RecordingError,
                    settings.play_sounds,
                    self.paths.as_ref(),
                );
                Err(error)
            }
        }
    }

    fn start_hotkey_monitor(&self) -> anyhow::Result<Value> {
        let settings = windows_settings_or_default(self.settings.clone());
        validate_shortcut_pair(
            &settings.push_to_talk_shortcut,
            &settings.hands_free_shortcut,
        )?;

        let action_tx = self.hotkey_runtime.start_worker(
            self.event_tx.clone(),
            self.hotkeys.clone(),
            self.audio.clone(),
            self.speech.clone(),
            self.cleanup.clone(),
        );

        match self.hotkeys.start(
            settings.push_to_talk_shortcut,
            settings.hands_free_shortcut,
            action_tx,
        ) {
            Ok(()) => Ok(json!({ "status": "hotkey-monitoring" })),
            Err(error) => {
                self.hotkey_runtime.stop_worker();
                self.emit_event(
                    NATIVE_CORE_EVENT_HOTKEY_MONITOR_FAILED,
                    json!({ "error": error.to_string() }),
                );
                Err(error)
            }
        }
    }

    fn stop_hotkey_monitor(&self) -> anyhow::Result<Value> {
        self.hotkeys.stop();
        self.hotkey_runtime.stop_worker();
        Ok(json!({ "status": "hotkey-stopped" }))
    }

    fn capture_shortcut(&self, payload: Value) -> anyhow::Result<Value> {
        let target_value = payload
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("captureShortcut payload is missing `target`"))?;
        let target = ShortcutCaptureTarget::try_from(target_value)?;
        let was_running = self.hotkeys.is_running();
        if was_running {
            self.hotkeys.stop();
            self.hotkey_runtime.stop_worker();
        }

        let captured = shortcut_capture::capture(target).and_then(|shortcut| {
            self.validate_captured_shortcut(target, &shortcut)?;
            Ok(shortcut)
        });

        let restart_result = if was_running {
            self.start_hotkey_monitor().map(|_| ())
        } else {
            Ok(())
        };

        match (captured, restart_result) {
            (Ok(shortcut), Ok(())) => serde_json::to_value(shortcut).map_err(Into::into),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error),
            (Err(capture_error), Err(restart_error)) => Err(anyhow::anyhow!(
                "{capture_error} Hotkey monitor did not restart: {restart_error}"
            )),
        }
    }

    fn validate_captured_shortcut(
        &self,
        target: ShortcutCaptureTarget,
        shortcut: &parrot_protocol::ShortcutSettings,
    ) -> anyhow::Result<()> {
        let settings = self.settings.clone().unwrap_or_default();
        let mut next = settings.clone();
        match target {
            ShortcutCaptureTarget::PushToTalk => {
                next.push_to_talk_shortcut = shortcut.clone();
            }
            ShortcutCaptureTarget::HandsFree => {
                next.hands_free_shortcut = shortcut.clone();
            }
        }
        validate_shortcut_pair(&next.push_to_talk_shortcut, &next.hands_free_shortcut)
    }

    fn emit_event(&self, event: &str, payload: Value) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event_message(event, payload));
        }
    }

    fn normalize_runtime_settings(&self, settings: &mut AppSettings) {
        normalize_settings_for_platform(settings, SettingsPlatform::Windows);
        clear_missing_selected_input_uid(settings, &self.audio);
    }
}

impl Drop for CoreService {
    fn drop(&mut self) {
        self.hotkeys.stop();
        self.hotkey_runtime.stop_worker();
    }
}

#[derive(Clone, Default)]
struct HotkeyRuntime {
    settings: Arc<Mutex<Option<AppSettings>>>,
    paths: Arc<Mutex<Option<NativeCorePaths>>>,
    cleanup_prompt: Arc<Mutex<String>>,
    debug_cleanup_failures: Arc<AtomicBool>,
    session: Arc<Mutex<HotkeySessionState>>,
    worker: Arc<Mutex<Option<HotkeyWorker>>>,
}

impl std::fmt::Debug for HotkeyRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HotkeyRuntime")
    }
}

struct HotkeyWorker {
    tx: mpsc::Sender<HotkeyAction>,
    join: Option<thread::JoinHandle<()>>,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Default)]
struct HotkeySessionState {
    recording: bool,
    active_source: Option<HotkeySource>,
    processing: bool,
    cancel_flag: Option<Arc<AtomicBool>>,
    paste_target: Option<PasteTarget>,
    generation: u64,
}

impl HotkeyRuntime {
    fn set_settings(&self, settings: Option<AppSettings>) {
        *self.settings.lock().expect("hotkey settings poisoned") = settings;
    }

    fn set_paths(&self, paths: Option<NativeCorePaths>) {
        *self.paths.lock().expect("hotkey paths poisoned") = paths;
    }

    fn set_cleanup_prompt(&self, cleanup_prompt: String) {
        *self
            .cleanup_prompt
            .lock()
            .expect("hotkey cleanup prompt poisoned") = cleanup_prompt;
    }

    fn set_debug_cleanup_failures(&self, enabled: bool) {
        self.debug_cleanup_failures.store(enabled, Ordering::SeqCst);
    }

    #[cfg(target_os = "windows")]
    fn start_worker(
        &self,
        event_tx: Option<mpsc::Sender<Value>>,
        hotkeys: HotkeyMonitor,
        audio: AudioManager,
        speech: WhisperCppPipeline,
        cleanup: LlamaCleanupPipeline,
    ) -> mpsc::Sender<HotkeyAction> {
        self.stop_worker();
        let (tx, rx) = mpsc::channel();
        let runtime = self.clone();
        let worker_tx = tx.clone();
        let join = thread::Builder::new()
            .name("Parrot Windows Hotkey Actions".into())
            .spawn(move || {
                runtime.worker_loop(rx, event_tx, hotkeys, audio, speech, cleanup);
            })
            .expect("failed to spawn hotkey action worker");
        *self.worker.lock().expect("hotkey worker poisoned") = Some(HotkeyWorker {
            tx: worker_tx,
            join: Some(join),
        });
        tx
    }

    #[cfg(not(target_os = "windows"))]
    fn start_worker(
        &self,
        _event_tx: Option<mpsc::Sender<Value>>,
        _hotkeys: HotkeyMonitor,
        _audio: AudioManager,
        _speech: WhisperCppPipeline,
        _cleanup: LlamaCleanupPipeline,
    ) -> mpsc::Sender<HotkeyAction> {
        self.stop_worker();
        let (tx, _rx) = mpsc::channel();
        *self.worker.lock().expect("hotkey worker poisoned") = Some(HotkeyWorker {
            tx: tx.clone(),
            join: None,
        });
        tx
    }

    fn worker_sender(&self) -> Option<mpsc::Sender<HotkeyAction>> {
        self.worker
            .lock()
            .expect("hotkey worker poisoned")
            .as_ref()
            .map(|worker| worker.tx.clone())
    }

    fn stop_worker(&self) {
        let worker = self.worker.lock().expect("hotkey worker poisoned").take();
        if let Some(worker) = worker {
            let _ = worker.tx.send(HotkeyAction::Shutdown);
            if let Some(join) = worker.join {
                let _ = join.join();
            }
        }
        let mut session = self.session.lock().expect("hotkey session poisoned");
        if let Some(cancel_flag) = session.cancel_flag.take() {
            cancel_flag.store(true, Ordering::SeqCst);
        }
        session.recording = false;
        session.processing = false;
        session.active_source = None;
        session.paste_target = None;
    }

    #[cfg(target_os = "windows")]
    fn worker_loop(
        &self,
        rx: mpsc::Receiver<HotkeyAction>,
        event_tx: Option<mpsc::Sender<Value>>,
        hotkeys: HotkeyMonitor,
        audio: AudioManager,
        speech: WhisperCppPipeline,
        cleanup: LlamaCleanupPipeline,
    ) {
        while let Ok(action) = rx.recv() {
            match action {
                HotkeyAction::Start { source } => {
                    self.handle_hotkey_start(source, &event_tx, &hotkeys, &audio, &speech);
                }
                HotkeyAction::Stop { source } => {
                    self.handle_hotkey_stop(source, &event_tx, &hotkeys, &audio, &speech, &cleanup);
                }
                HotkeyAction::Cancel => {
                    self.handle_hotkey_cancel(&event_tx, &hotkeys, &audio);
                }
                HotkeyAction::Shutdown => break,
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn handle_hotkey_start(
        &self,
        source: HotkeySource,
        event_tx: &Option<mpsc::Sender<Value>>,
        hotkeys: &HotkeyMonitor,
        audio: &AudioManager,
        speech: &WhisperCppPipeline,
    ) {
        let busy_event = {
            let session = self.session.lock().expect("hotkey session poisoned");
            if session.recording {
                Some(NATIVE_CORE_EVENT_RECORDING_STARTED)
            } else if session.processing {
                Some(NATIVE_CORE_EVENT_RECORDING_PROCESSING)
            } else {
                None
            }
        };

        if let Some(event_name) = busy_event {
            hotkeys.force_toggle_off(source);
            emit_event(
                event_tx,
                event_name,
                json!({ "kind": "dictation", "source": source.as_str(), "busy": true }),
            );
            return;
        }

        let settings = self.current_settings();
        let paste_target = if settings.paste_into_recording_start_window {
            paste::capture_current_target()
        } else {
            None
        };
        match audio.start_recording(settings.selected_input_uid.as_deref()) {
            Ok(()) => {
                {
                    let mut session = self.session.lock().expect("hotkey session poisoned");
                    session.generation += 1;
                    session.recording = true;
                    session.processing = false;
                    session.active_source = Some(source);
                    session.cancel_flag = None;
                    session.paste_target = paste_target;
                }
                hotkeys.set_cancellation_enabled(true);
                emit_event(
                    event_tx,
                    NATIVE_CORE_EVENT_RECORDING_STARTED,
                    json!({ "kind": "dictation", "source": source.as_str() }),
                );
                let sound_paths = self.current_paths();
                sound::play(
                    SoundEvent::RecordingStart,
                    settings.play_sounds,
                    sound_paths.as_ref(),
                );

                if let Some(paths) = sound_paths {
                    let warm_settings = settings;
                    let warm_speech = speech.clone();
                    thread::spawn(move || {
                        if let Err(error) = warm_speech.warm(&warm_settings, &paths) {
                            eprintln!("Windows whisper.cpp warmup failed: {error}");
                        }
                    });
                }
            }
            Err(error) => {
                hotkeys.force_toggle_off(source);
                hotkeys.set_cancellation_enabled(false);
                let sound_paths = self.current_paths();
                sound::play(
                    SoundEvent::RecordingError,
                    settings.play_sounds,
                    sound_paths.as_ref(),
                );
                emit_event(
                    event_tx,
                    NATIVE_CORE_EVENT_RECORDING_FAILED,
                    json!({ "error": error.to_string() }),
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn handle_hotkey_stop(
        &self,
        source: HotkeySource,
        event_tx: &Option<mpsc::Sender<Value>>,
        hotkeys: &HotkeyMonitor,
        audio: &AudioManager,
        speech: &WhisperCppPipeline,
        cleanup: &LlamaCleanupPipeline,
    ) {
        let (generation, cancel_flag, paste_target) = {
            let mut session = self.session.lock().expect("hotkey session poisoned");
            if !session.recording || session.active_source != Some(source) {
                return;
            }
            session.generation += 1;
            session.recording = false;
            session.processing = true;
            session.active_source = None;
            let cancel_flag = Arc::new(AtomicBool::new(false));
            session.cancel_flag = Some(cancel_flag.clone());
            let paste_target = session.paste_target.take();
            (session.generation, cancel_flag, paste_target)
        };

        emit_event(
            event_tx,
            NATIVE_CORE_EVENT_RECORDING_PROCESSING,
            json!({ "kind": "dictation", "source": source.as_str() }),
        );

        let recorded = audio.stop_recording();
        let runtime = self.clone();
        let event_tx = event_tx.clone();
        let hotkeys = hotkeys.clone();
        let speech = speech.clone();
        let cleanup = cleanup.clone();
        thread::spawn(move || {
            let result = recorded.and_then(|recorded| {
                runtime.finish_hotkey_recording(recorded, &speech, &cleanup, &cancel_flag)
            });
            runtime.finish_hotkey_processing(
                generation,
                cancel_flag,
                paste_target,
                result,
                &event_tx,
                &hotkeys,
            );
        });
    }

    #[cfg(target_os = "windows")]
    fn handle_hotkey_cancel(
        &self,
        event_tx: &Option<mpsc::Sender<Value>>,
        hotkeys: &HotkeyMonitor,
        audio: &AudioManager,
    ) {
        let was_active = {
            let mut session = self.session.lock().expect("hotkey session poisoned");
            let was_active =
                session.recording || session.processing || session.cancel_flag.is_some();
            if let Some(cancel_flag) = session.cancel_flag.take() {
                cancel_flag.store(true, Ordering::SeqCst);
            }
            session.generation += 1;
            session.recording = false;
            session.processing = false;
            session.active_source = None;
            session.paste_target = None;
            was_active
        };

        if !was_active {
            return;
        }

        let _ = audio.stop_recording();
        hotkeys.set_cancellation_enabled(false);
        let settings = self.current_settings();
        let sound_paths = self.current_paths();
        sound::play(
            SoundEvent::RecordingCancel,
            settings.play_sounds,
            sound_paths.as_ref(),
        );
        emit_event(
            event_tx,
            NATIVE_CORE_EVENT_RECORDING_CANCELLED,
            json!({ "kind": "dictation" }),
        );
    }

    #[cfg(target_os = "windows")]
    fn finish_hotkey_recording(
        &self,
        recorded: RecordedAudio,
        speech: &WhisperCppPipeline,
        cleanup: &LlamaCleanupPipeline,
        cancel_flag: &Arc<AtomicBool>,
    ) -> anyhow::Result<RecordingResult> {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Recording cancelled."));
        }
        let settings = self.current_settings();
        let paths = self
            .current_paths()
            .ok_or_else(|| anyhow::anyhow!("native-core paths are not initialized"))?;
        let transcription = speech.transcribe_with_cancel(
            &recorded.samples_16khz,
            &settings,
            &paths,
            cancel_flag.clone(),
        )?;
        if cancel_flag.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Recording cancelled."));
        }
        let cleanup_prompt = self
            .cleanup_prompt
            .lock()
            .expect("hotkey cleanup prompt poisoned")
            .clone();
        let cleaned = cleanup.cleanup_with_cancel(
            &transcription.text,
            &settings,
            &transcription.language,
            &paths.cleanup_models_dir,
            &cleanup_prompt,
            self.debug_cleanup_failures.load(Ordering::SeqCst),
            cancel_flag,
        )?;
        Ok(RecordingResult {
            raw: transcription.text,
            cleaned,
            audio_duration_seconds: recorded.duration_seconds,
        })
    }

    #[cfg(target_os = "windows")]
    fn finish_hotkey_processing(
        &self,
        generation: u64,
        cancel_flag: Arc<AtomicBool>,
        paste_target: Option<PasteTarget>,
        result: anyhow::Result<RecordingResult>,
        event_tx: &Option<mpsc::Sender<Value>>,
        hotkeys: &HotkeyMonitor,
    ) {
        let cancelled = cancel_flag.load(Ordering::SeqCst);
        let should_emit = {
            let mut session = self.session.lock().expect("hotkey session poisoned");
            let is_current = session.generation == generation;
            if is_current {
                session.processing = false;
                session.cancel_flag = None;
            }
            is_current && !cancelled
        };

        if !should_emit {
            return;
        }

        hotkeys.set_cancellation_enabled(false);
        let settings = self.current_settings();
        let sound_paths = self.current_paths();
        match result {
            Ok(result) => {
                let paste_error = if result.cleaned.trim().is_empty() {
                    None
                } else {
                    paste::paste_text(&result.cleaned, paste_target.as_ref())
                        .err()
                        .map(|error| error.to_string())
                };
                sound::play(
                    if paste_error.is_none() {
                        SoundEvent::RecordingSuccess
                    } else {
                        SoundEvent::RecordingError
                    },
                    settings.play_sounds,
                    sound_paths.as_ref(),
                );
                let mut payload = serde_json::to_value(result).unwrap_or_else(|_| json!({}));
                payload["kind"] = json!("dictation");
                if let Some(error) = paste_error {
                    payload["pasteError"] = json!(error);
                }
                emit_event(event_tx, NATIVE_CORE_EVENT_RECORDING_FINISHED, payload);
            }
            Err(error) => {
                sound::play(
                    SoundEvent::RecordingError,
                    settings.play_sounds,
                    sound_paths.as_ref(),
                );
                emit_event(
                    event_tx,
                    NATIVE_CORE_EVENT_RECORDING_FAILED,
                    json!({ "error": error.to_string() }),
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn current_settings(&self) -> AppSettings {
        windows_settings_or_default(
            self.settings
                .lock()
                .expect("hotkey settings poisoned")
                .clone(),
        )
    }

    #[cfg(target_os = "windows")]
    fn current_paths(&self) -> Option<NativeCorePaths> {
        self.paths.lock().expect("hotkey paths poisoned").clone()
    }
}

#[cfg(target_os = "windows")]
fn emit_event(event_tx: &Option<mpsc::Sender<Value>>, event: &str, payload: Value) {
    if let Some(tx) = event_tx {
        let _ = tx.send(event_message(event, payload));
    }
}

fn windows_settings_or_default(settings: Option<AppSettings>) -> AppSettings {
    let mut settings = settings.unwrap_or_default();
    normalize_settings_for_platform(&mut settings, SettingsPlatform::Windows);
    settings
}

#[cfg(target_os = "windows")]
fn clear_missing_selected_input_uid(settings: &mut AppSettings, audio: &AudioManager) {
    let Some(selected_uid) = settings.selected_input_uid.as_deref() else {
        return;
    };
    let Ok(devices) = audio.list_input_devices() else {
        settings.selected_input_uid = None;
        return;
    };
    if !selected_uid_exists(&devices, selected_uid) {
        settings.selected_input_uid = None;
    }
}

#[cfg(not(target_os = "windows"))]
fn clear_missing_selected_input_uid(_settings: &mut AppSettings, _audio: &AudioManager) {}

#[cfg(test)]
mod tests {
    use super::*;
    use parrot_protocol::{
        default_push_to_talk_shortcut, default_windows_push_to_talk_shortcut, DictationLanguageMode,
    };
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

    #[test]
    fn initialize_stores_settings_paths_cleanup_prompt_and_debug_flags() {
        let mut service = CoreService::default();
        let paths = NativeCorePaths {
            app_data_dir: "C:/Users/Alice/AppData/Roaming/in.basic.parrot".into(),
            models_dir: "C:/Users/Alice/AppData/Local/Parrot/Models".into(),
            speech_models_dir: "C:/Users/Alice/AppData/Local/Parrot/Models/whisper-models".into(),
            cleanup_models_dir: "C:/Users/Alice/AppData/Local/Parrot/Models/cleanup-models".into(),
            resources_dir: "C:/Program Files/Parrot/resources".into(),
            shared_resources_dir: "C:/Program Files/Parrot/resources/native-core/shared".into(),
            temp_dir: "C:/Users/Alice/AppData/Local/Temp".into(),
        };
        let payload = json!({
            "settings": AppSettings {
                push_to_talk_shortcut: default_push_to_talk_shortcut(),
                ..AppSettings::default()
            },
            "paths": paths,
            "prompts": {
                "cleanupDefaultInstructions": "Clean this transcript."
            },
            "debugCleanupFailures": true,
            "languageCatalog": [
                { "code": "en", "name": "English" }
            ]
        });

        let response = service.handle_request(RequestLine {
            id: "init".into(),
            method: "initialize".into(),
            payload,
        });

        assert_eq!(response["ok"], true);
        assert_eq!(response["payload"]["status"], "initialized");
        assert_eq!(
            service.settings.as_ref().unwrap().push_to_talk_shortcut,
            default_windows_push_to_talk_shortcut()
        );
        assert!(service.paths.is_some());
        assert_eq!(service.cleanup_prompt, "Clean this transcript.");
        assert!(service.debug_cleanup_failures);
    }

    #[test]
    fn permission_statuses_returns_windows_microphone_snapshot() {
        let mut service = CoreService::default();
        let response = service.handle_request(RequestLine {
            id: "permissions".into(),
            method: "permissionStatuses".into(),
            payload: json!({}),
        });

        assert_eq!(response["ok"], true);
        assert_eq!(
            response["payload"]["requirements"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(response["payload"]["requirements"][0]["kind"], "microphone");
        let state = response["payload"]["requirements"][0]["state"]
            .as_str()
            .unwrap();
        assert!(matches!(state, "unknown" | "granted" | "denied"));
        assert_eq!(response["payload"]["accessibility"], Value::Null);
        assert_eq!(response["payload"]["inputMonitoring"], Value::Null);
    }

    #[test]
    fn model_statuses_reports_windows_catalog_and_required_flags() {
        let temp = TempDir::new().unwrap();
        let mut service = CoreService::default();
        let init = service.handle_request(RequestLine {
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
        });
        assert_eq!(init["ok"], true);

        let response = service.handle_request(RequestLine {
            id: "models".into(),
            method: "modelStatuses".into(),
            payload: json!({}),
        });

        assert_eq!(response["ok"], true);
        let models = response["payload"].as_array().unwrap();
        assert_eq!(models.len(), 4);
        assert_eq!(models[0]["id"], "speech");
        assert_eq!(models[0]["role"], "speech");
        assert_eq!(models[1]["id"], "speech-multilingual");
        assert_eq!(models[1]["required"], true);
        assert_eq!(models[2]["id"], "cleanup");
        assert_eq!(models[3]["id"], "cleanup-gemma-4-e2b");
        assert_eq!(models[3]["required"], true);
    }

    #[test]
    fn unknown_methods_return_structured_errors() {
        let mut service = CoreService::default();
        let response = service.handle_request(RequestLine {
            id: "unknown".into(),
            method: "bogus".into(),
            payload: json!({}),
        });

        assert_eq!(response["id"], "unknown");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"], "Unknown native-core method: bogus");
    }

    #[test]
    fn hotkey_monitor_methods_return_structured_status() {
        let mut service = CoreService::default();
        let start = service.handle_request(RequestLine {
            id: "hotkeys".into(),
            method: "startHotkeyMonitor".into(),
            payload: json!({}),
        });

        assert_eq!(start["ok"], true);
        assert_eq!(start["payload"]["status"], "hotkey-monitoring");

        let stop = service.handle_request(RequestLine {
            id: "stop-hotkeys".into(),
            method: "stopHotkeyMonitor".into(),
            payload: json!({}),
        });

        assert_eq!(stop["ok"], true);
        assert_eq!(stop["payload"]["status"], "hotkey-stopped");
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn shortcut_capture_reports_platform_unavailable_on_non_windows() {
        let mut service = CoreService::default();
        let response = service.handle_request(RequestLine {
            id: "capture".into(),
            method: "captureShortcut".into(),
            payload: json!({ "target": "pushToTalkShortcut" }),
        });

        assert_eq!(response["ok"], false);
        assert_eq!(
            response["error"],
            "Windows shortcut capture is unavailable on this platform."
        );
    }
}
