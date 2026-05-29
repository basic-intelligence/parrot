#![cfg_attr(test, allow(dead_code))]

use anyhow::{anyhow, Context};
use parrot_protocol::{
    NativeCoreMethod, NATIVE_CORE_EVENT_DISCONNECTED, NATIVE_CORE_EVENT_RECORDING_CANCELLED,
    NATIVE_CORE_EVENT_RECORDING_FAILED, NATIVE_CORE_EVENT_RECORDING_FINISHED,
    NATIVE_CORE_EVENT_RECORDING_PROCESSING, NATIVE_CORE_EVENT_RECORDING_STARTED,
    NATIVE_CORE_EVENT_RECOVERED,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::ErrorKind,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
#[cfg(target_os = "windows")]
use tauri::PhysicalPosition;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(target_os = "linux")]
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::{
    process::{CommandChild, CommandEvent},
    ShellExt,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

struct PendingRequest {
    generation: u64,
    tx: oneshot::Sender<anyhow::Result<Value>>,
}

enum ParsedStdoutLine {
    Event {
        name: String,
        payload: Value,
    },
    Response {
        id: String,
        result: anyhow::Result<Value>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayAction {
    Show,
    HideAfter(Duration),
    None,
}

static OVERLAY_GENERATION: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "windows")]
const RECORDING_OVERLAY_WIDTH: u32 = 148;
#[cfg(target_os = "windows")]
const RECORDING_OVERLAY_HEIGHT: u32 = 36;
#[cfg(target_os = "windows")]
const RECORDING_OVERLAY_BOTTOM_MARGIN: i32 = 96;

const CPU_SIDECAR: &str = "parrot-core-cpu";
#[cfg(not(target_os = "linux"))]
const CUDA_SIDECAR: &str = "parrot-core-cuda";

#[derive(Clone)]
pub struct CoreBridge {
    app: AppHandle,
    child: Arc<Mutex<Option<CommandChild>>>,
    pending: Arc<Mutex<HashMap<String, PendingRequest>>>,
    generation: Arc<AtomicU64>,
    reconnect_lock: Arc<Mutex<()>>,
}

impl CoreBridge {
    pub async fn spawn(app: AppHandle) -> anyhow::Result<Self> {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let child = Arc::new(Mutex::new(None));
        let generation = Arc::new(AtomicU64::new(0));
        let sidecar_child = Self::spawn_sidecar(
            app.clone(),
            child.clone(),
            pending.clone(),
            generation.clone(),
        )
        .await?;

        *child.lock().await = Some(sidecar_child);

        Ok(Self {
            app,
            child,
            pending,
            generation,
            reconnect_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn app(&self) -> &AppHandle {
        &self.app
    }

    pub async fn reconnect(&self) -> anyhow::Result<()> {
        let _guard = self.reconnect_lock.lock().await;

        prepare_for_reconnect(&self.pending, &self.child).await;

        let sidecar_child = Self::spawn_sidecar(
            self.app.clone(),
            self.child.clone(),
            self.pending.clone(),
            self.generation.clone(),
        )
        .await?;

        *self.child.lock().await = Some(sidecar_child);

        let _ = self.app.emit(
            NATIVE_CORE_EVENT_RECOVERED,
            json!({ "status": "Parrot Core reconnected." }),
        );

        Ok(())
    }

    async fn spawn_sidecar(
        app: AppHandle,
        child: Arc<Mutex<Option<CommandChild>>>,
        pending: Arc<Mutex<HashMap<String, PendingRequest>>>,
        generation: Arc<AtomicU64>,
    ) -> anyhow::Result<CommandChild> {
        let connection_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut last_error: Option<anyhow::Error> = None;

        for sidecar_name in preferred_sidecars() {
            match Self::spawn_named_sidecar(
                app.clone(),
                child.clone(),
                pending.clone(),
                generation.clone(),
                connection_generation,
                sidecar_name,
            )
            .await
            {
                Ok(sidecar_child) => return Ok(sidecar_child),
                Err(error) => {
                    eprintln!("failed to spawn {sidecar_name}: {error}");
                    last_error = Some(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("failed to spawn any native-core sidecar")))
    }

    async fn spawn_named_sidecar(
        app: AppHandle,
        child: Arc<Mutex<Option<CommandChild>>>,
        pending: Arc<Mutex<HashMap<String, PendingRequest>>>,
        generation: Arc<AtomicU64>,
        connection_generation: u64,
        sidecar_name: &'static str,
    ) -> anyhow::Result<CommandChild> {
        let sidecar_command = app
            .shell()
            .sidecar(sidecar_name)
            .with_context(|| format!("failed to create {sidecar_name} sidecar command"))?;
        #[cfg(target_os = "linux")]
        let sidecar_command = configure_linux_sidecar_library_path(&app, sidecar_command);

        let (mut rx, sidecar_child) = sidecar_command
            .spawn()
            .with_context(|| format!("failed to spawn {sidecar_name} sidecar"))?;

        let (stdout_tx, mut stdout_rx) = mpsc::unbounded_channel::<String>();
        let (stderr_tx, mut stderr_rx) = mpsc::unbounded_channel::<String>();

        let app_for_stdout = app.clone();
        let pending_for_stdout = pending.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(line) = stdout_rx.recv().await {
                let line = line.trim();
                if !line.is_empty() {
                    process_stdout_line(&app_for_stdout, &pending_for_stdout, line).await;
                }
            }
        });

        let sidecar_name_for_stderr = sidecar_name.to_string();
        tauri::async_runtime::spawn(async move {
            while let Some(line) = stderr_rx.recv().await {
                let line = line.trim();
                if !line.is_empty() {
                    eprintln!("{sidecar_name_for_stderr} stderr: {line}");
                }
            }
        });

        let app_for_events = app.clone();
        let child_for_close = child.clone();
        let generation_for_close = generation.clone();
        let sidecar_name_for_close = sidecar_name.to_string();
        tauri::async_runtime::spawn(async move {
            let close_message: String;
            loop {
                match rx.recv().await {
                    Some(CommandEvent::Stdout(bytes)) => {
                        if let Ok(line) = String::from_utf8(bytes) {
                            let _ = stdout_tx.send(line);
                        }
                    }
                    Some(CommandEvent::Stderr(bytes)) => {
                        if let Ok(line) = String::from_utf8(bytes) {
                            let _ = stderr_tx.send(line);
                        }
                    }
                    Some(CommandEvent::Error(error)) => {
                        close_message =
                            format!("{sidecar_name_for_close} sidecar stream error: {error}");
                        eprintln!("{close_message}");
                        break;
                    }
                    Some(CommandEvent::Terminated(payload)) => {
                        close_message = format!(
                            "{sidecar_name_for_close} sidecar exited with code {:?}",
                            payload.code
                        );
                        eprintln!("{close_message}");
                        break;
                    }
                    None => {
                        close_message =
                            format!("{sidecar_name_for_close} sidecar event stream closed");
                        eprintln!("{close_message}");
                        break;
                    }
                    _ => {}
                }
            }

            fail_pending_generation(&pending, connection_generation, close_message.clone()).await;

            if generation_for_close.load(Ordering::SeqCst) == connection_generation {
                *child_for_close.lock().await = None;
                hide_recording_overlay(&app_for_events);
                let _ = app_for_events.emit(
                    NATIVE_CORE_EVENT_DISCONNECTED,
                    json!({ "error": close_message }),
                );
            }
        });

        Ok(sidecar_child)
    }

    pub async fn request(&self, method: NativeCoreMethod, payload: Value) -> anyhow::Result<Value> {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let request_generation = self.generation.load(Ordering::SeqCst);
        self.pending.lock().await.insert(
            id.clone(),
            PendingRequest {
                generation: request_generation,
                tx,
            },
        );

        let method_name = method.as_str();
        let line =
            json!({ "id": id, "method": method_name, "payload": payload }).to_string() + "\n";
        let write_result = {
            let mut child = self.child.lock().await;
            match child.as_mut() {
                Some(child) => Some(child.write(line.as_bytes())),
                None => None,
            }
        };

        match write_result {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                self.pending.lock().await.remove(&id);
                return Err(error).with_context(|| {
                    format!("failed to write native core request `{method_name}`")
                });
            }
            None => {
                self.pending.lock().await.remove(&id);
                return Err(anyhow!("native core sidecar is not connected"));
            }
        }

        wait_for_response(
            &self.pending,
            &id,
            method_name,
            rx,
            Duration::from_secs(300),
        )
        .await
    }
}

#[cfg(target_os = "linux")]
fn preferred_sidecars() -> Vec<&'static str> {
    vec![CPU_SIDECAR]
}

#[cfg(not(target_os = "linux"))]
fn preferred_sidecars() -> Vec<&'static str> {
    if let Ok(value) = std::env::var("PARROT_WINDOWS_CORE_BACKEND") {
        match value.trim().to_ascii_lowercase().as_str() {
            "cpu" => return vec![CPU_SIDECAR],
            "cuda" => return vec![CUDA_SIDECAR, CPU_SIDECAR],
            _ => {}
        }
    }

    if std::env::var_os("PARROT_FORCE_CPU").is_some() {
        return vec![CPU_SIDECAR];
    }

    if windows_has_cuda_driver() {
        vec![CUDA_SIDECAR, CPU_SIDECAR]
    } else {
        vec![CPU_SIDECAR]
    }
}

#[cfg(target_os = "linux")]
fn configure_linux_sidecar_library_path(app: &AppHandle, command: Command) -> Command {
    use std::path::PathBuf;

    let mut paths = Vec::<PathBuf>::new();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            paths.push(exe_dir.to_path_buf());
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        paths.push(resource_dir.join("binaries"));
        paths.push(resource_dir);
    }

    if let Ok(current_dir) = std::env::current_dir() {
        paths.push(current_dir.join("src-tauri").join("binaries"));
    }

    if let Some(existing_path) = std::env::var_os("LD_LIBRARY_PATH") {
        paths.extend(std::env::split_paths(&existing_path));
    }

    let existing_paths = paths
        .into_iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();

    match std::env::join_paths(existing_paths) {
        Ok(library_path) => command.env("LD_LIBRARY_PATH", library_path),
        Err(error) => {
            eprintln!("failed to configure sidecar library path: {error}");
            command
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_has_cuda_driver() -> bool {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{Foundation::FreeLibrary, System::LibraryLoader::LoadLibraryW};

    let library_name = OsStr::new("nvcuda.dll")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    let handle = unsafe { LoadLibraryW(library_name.as_ptr()) };
    if handle.is_null() {
        return false;
    }

    unsafe {
        FreeLibrary(handle);
    }

    true
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
fn windows_has_cuda_driver() -> bool {
    false
}

async fn prepare_for_reconnect(
    pending: &Arc<Mutex<HashMap<String, PendingRequest>>>,
    child: &Arc<Mutex<Option<CommandChild>>>,
) {
    fail_all_pending(pending, "native core reconnecting").await;

    if let Some(child) = child.lock().await.take() {
        let _ = child.kill();
    }
}

pub fn is_native_core_disconnect(error: &anyhow::Error) -> bool {
    for cause in error.chain() {
        if let Some(io_error) = cause.downcast_ref::<std::io::Error>() {
            if matches!(
                io_error.kind(),
                ErrorKind::BrokenPipe
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::ConnectionReset
                    | ErrorKind::NotConnected
                    | ErrorKind::UnexpectedEof
            ) {
                return true;
            }
        }
    }

    let message = error.to_string().to_lowercase();
    message.contains("broken pipe")
        || message.contains("sidecar is not connected")
        || message.contains("sidecar exited")
        || message.contains("stream closed")
        || message.contains("response channel closed")
        || message.contains("native core reconnecting")
}

async fn fail_all_pending(
    pending: &Arc<Mutex<HashMap<String, PendingRequest>>>,
    message: impl Into<String>,
) {
    let message = message.into();
    let senders = {
        let mut pending = pending.lock().await;
        pending
            .drain()
            .map(|(_, pending)| pending.tx)
            .collect::<Vec<_>>()
    };

    for tx in senders {
        let _ = tx.send(Err(anyhow!(message.clone())));
    }
}

async fn fail_pending_generation(
    pending: &Arc<Mutex<HashMap<String, PendingRequest>>>,
    generation: u64,
    message: impl Into<String>,
) {
    let message = message.into();
    let senders = {
        let mut pending = pending.lock().await;
        let ids = pending
            .iter()
            .filter_map(|(id, request)| (request.generation == generation).then(|| id.clone()))
            .collect::<Vec<_>>();

        ids.into_iter()
            .filter_map(|id| pending.remove(&id).map(|request| request.tx))
            .collect::<Vec<_>>()
    };

    for tx in senders {
        let _ = tx.send(Err(anyhow!(message.clone())));
    }
}

async fn wait_for_response(
    pending: &Arc<Mutex<HashMap<String, PendingRequest>>>,
    id: &str,
    method_name: &str,
    rx: oneshot::Receiver<anyhow::Result<Value>>,
    timeout: Duration,
) -> anyhow::Result<Value> {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(anyhow!("native core response channel closed")),
        Err(_) => {
            pending.lock().await.remove(id);
            Err(anyhow!("native core timed out on {method_name}"))
        }
    }
}

async fn process_stdout_line(
    app: &AppHandle,
    pending: &Arc<Mutex<HashMap<String, PendingRequest>>>,
    line: &str,
) {
    let parsed = match parse_stdout_line(line) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("native-core invalid stdout line: {error}; line={line}");
            return;
        }
    };

    match parsed {
        ParsedStdoutLine::Event { name, payload } => handle_native_event(app, &name, payload),
        ParsedStdoutLine::Response { id, result } => {
            if let Some(request) = pending.lock().await.remove(&id) {
                let _ = request.tx.send(result);
            }
        }
    }
}

fn parse_stdout_line(line: &str) -> anyhow::Result<ParsedStdoutLine> {
    let parsed: Value = serde_json::from_str(line).context("invalid JSON")?;

    if let Some(event_name) = parsed.get("event").and_then(|v| v.as_str()) {
        let payload = parsed.get("payload").cloned().unwrap_or(Value::Null);
        return Ok(ParsedStdoutLine::Event {
            name: event_name.to_string(),
            payload,
        });
    }

    let id = parsed
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("native-core response missing id"))?
        .to_string();
    let ok = parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let result = if ok {
        Ok(parsed.get("payload").cloned().unwrap_or(Value::Null))
    } else {
        let error = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("native core error")
            .to_string();
        Err(anyhow!(error))
    };

    Ok(ParsedStdoutLine::Response { id, result })
}

fn handle_native_event(app: &AppHandle, event_name: &str, payload: Value) {
    match overlay_action_for_event(event_name) {
        OverlayAction::Show => show_recording_overlay(app),
        OverlayAction::HideAfter(delay) => hide_recording_overlay_after(app.clone(), delay),
        OverlayAction::None => {}
    }
    let _ = app.emit(event_name, payload);
}

fn overlay_action_for_event(event_name: &str) -> OverlayAction {
    match event_name {
        NATIVE_CORE_EVENT_RECORDING_STARTED => OverlayAction::Show,
        NATIVE_CORE_EVENT_RECORDING_PROCESSING => OverlayAction::Show,
        NATIVE_CORE_EVENT_RECORDING_FINISHED => {
            OverlayAction::HideAfter(Duration::from_millis(120))
        }
        NATIVE_CORE_EVENT_RECORDING_FAILED => OverlayAction::HideAfter(Duration::from_millis(700)),
        NATIVE_CORE_EVENT_RECORDING_CANCELLED => {
            OverlayAction::HideAfter(Duration::from_millis(180))
        }
        _ => OverlayAction::None,
    }
}

#[cfg(target_os = "linux")]
fn show_recording_overlay(app: &AppHandle) {
    OVERLAY_GENERATION.fetch_add(1, Ordering::SeqCst);
    crate::core_bridge::overlay_linux::show_recording_overlay(app);
}

#[cfg(target_os = "windows")]
fn show_recording_overlay(app: &AppHandle) {
    OVERLAY_GENERATION.fetch_add(1, Ordering::SeqCst);
    let Some(window) = app.get_webview_window("recording") else {
        eprintln!("recording overlay window not found");
        return;
    };
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        let width = window
            .outer_size()
            .map(|s| s.width as i32)
            .unwrap_or(RECORDING_OVERLAY_WIDTH as i32);
        let height = window
            .outer_size()
            .map(|s| s.height as i32)
            .unwrap_or(RECORDING_OVERLAY_HEIGHT as i32);
        let x = monitor_pos.x + ((monitor_size.width as i32 - width) / 2);
        let y =
            monitor_pos.y + monitor_size.height as i32 - height - RECORDING_OVERLAY_BOTTOM_MARGIN;
        if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
            eprintln!("failed to position recording overlay: {error}");
        }
    }
    if let Err(error) = window.unminimize() {
        eprintln!("failed to unminimize recording overlay: {error}");
    }
    if let Err(error) = window.set_always_on_top(true) {
        eprintln!("failed to keep recording overlay above other windows: {error}");
    }
    if let Err(error) = window.show() {
        eprintln!("failed to show recording overlay: {error}");
    }
}

#[cfg(all(test, not(target_os = "linux"), not(target_os = "windows")))]
fn show_recording_overlay(_app: &AppHandle) {
    OVERLAY_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn hide_recording_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window("recording") else {
        return;
    };
    let _ = window.hide();
}

fn hide_recording_overlay_after(app: AppHandle, delay: Duration) {
    let generation = OVERLAY_GENERATION.load(Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        if OVERLAY_GENERATION.load(Ordering::SeqCst) == generation {
            hide_recording_overlay(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_line() {
        let parsed = parse_stdout_line(r#"{"id":"one","ok":true,"payload":{"ready":true}}"#)
            .expect("response should parse");

        match parsed {
            ParsedStdoutLine::Response { id, result } => {
                assert_eq!(id, "one");
                assert_eq!(result.unwrap(), json!({ "ready": true }));
            }
            ParsedStdoutLine::Event { .. } => panic!("expected response"),
        }
    }

    #[test]
    fn parses_event_line() {
        let parsed = parse_stdout_line(
            r#"{"event":"parrot:recording-started","payload":{"kind":"normal"}}"#,
        )
        .expect("event should parse");

        match parsed {
            ParsedStdoutLine::Event { name, payload } => {
                assert_eq!(name, NATIVE_CORE_EVENT_RECORDING_STARTED);
                assert_eq!(payload, json!({ "kind": "normal" }));
            }
            ParsedStdoutLine::Response { .. } => panic!("expected event"),
        }
    }

    #[test]
    fn parses_error_response_line() {
        let parsed = parse_stdout_line(r#"{"id":"two","ok":false,"error":"denied"}"#)
            .expect("error response should parse");

        match parsed {
            ParsedStdoutLine::Response { id, result } => {
                assert_eq!(id, "two");
                assert_eq!(result.unwrap_err().to_string(), "denied");
            }
            ParsedStdoutLine::Event { .. } => panic!("expected response"),
        }
    }

    #[test]
    fn classifies_native_core_disconnect_errors() {
        let broken_pipe = anyhow!(std::io::Error::from(ErrorKind::BrokenPipe))
            .context("failed to write native core request `captureShortcut`");
        assert!(is_native_core_disconnect(&broken_pipe));

        let stream_closed = anyhow!("native-core sidecar event stream closed");
        assert!(is_native_core_disconnect(&stream_closed));

        let timeout = anyhow!("native core timed out on modelStatuses");
        assert!(!is_native_core_disconnect(&timeout));
    }

    #[test]
    fn fails_only_pending_requests_for_closed_generation() {
        tauri::async_runtime::block_on(async {
            let pending = Arc::new(Mutex::new(HashMap::new()));
            let (tx_one, rx_one) = oneshot::channel();
            let (tx_two, rx_two) = oneshot::channel();

            pending.lock().await.insert(
                "one".to_string(),
                PendingRequest {
                    generation: 1,
                    tx: tx_one,
                },
            );
            pending.lock().await.insert(
                "two".to_string(),
                PendingRequest {
                    generation: 2,
                    tx: tx_two,
                },
            );

            fail_pending_generation(&pending, 1, "sidecar exited").await;

            assert!(rx_one.await.unwrap().is_err());
            assert!(pending.lock().await.contains_key("two"));

            fail_all_pending(&pending, "native core reconnecting").await;
            assert!(rx_two.await.unwrap().is_err());
            assert!(pending.lock().await.is_empty());
        });
    }

    #[test]
    fn pending_request_timeout_removes_pending_entry() {
        tauri::async_runtime::block_on(async {
            let pending = Arc::new(Mutex::new(HashMap::new()));
            let (tx, rx) = oneshot::channel();
            pending
                .lock()
                .await
                .insert("one".to_string(), PendingRequest { generation: 1, tx });

            let error = wait_for_response(
                &pending,
                "one",
                "modelStatuses",
                rx,
                Duration::from_millis(1),
            )
            .await
            .expect_err("request should time out");

            assert_eq!(error.to_string(), "native core timed out on modelStatuses");
            assert!(pending.lock().await.is_empty());
        });
    }

    #[test]
    fn reconnect_preparation_fails_stale_pending_requests() {
        tauri::async_runtime::block_on(async {
            let pending = Arc::new(Mutex::new(HashMap::new()));
            let child = Arc::new(Mutex::new(None));
            let (tx_one, rx_one) = oneshot::channel();
            let (tx_two, rx_two) = oneshot::channel();

            pending.lock().await.insert(
                "one".to_string(),
                PendingRequest {
                    generation: 1,
                    tx: tx_one,
                },
            );
            pending.lock().await.insert(
                "two".to_string(),
                PendingRequest {
                    generation: 2,
                    tx: tx_two,
                },
            );

            prepare_for_reconnect(&pending, &child).await;

            assert_eq!(
                rx_one.await.unwrap().unwrap_err().to_string(),
                "native core reconnecting"
            );
            assert_eq!(
                rx_two.await.unwrap().unwrap_err().to_string(),
                "native core reconnecting"
            );
            assert!(pending.lock().await.is_empty());
            assert!(child.lock().await.is_none());
        });
    }

    #[test]
    fn maps_recording_events_to_overlay_actions() {
        assert_eq!(
            overlay_action_for_event(NATIVE_CORE_EVENT_RECORDING_STARTED),
            OverlayAction::Show
        );
        assert_eq!(
            overlay_action_for_event(NATIVE_CORE_EVENT_RECORDING_PROCESSING),
            OverlayAction::Show
        );
        assert_eq!(
            overlay_action_for_event(NATIVE_CORE_EVENT_RECORDING_FINISHED),
            OverlayAction::HideAfter(Duration::from_millis(120))
        );
        assert_eq!(
            overlay_action_for_event(NATIVE_CORE_EVENT_RECORDING_FAILED),
            OverlayAction::HideAfter(Duration::from_millis(700))
        );
        assert_eq!(
            overlay_action_for_event(NATIVE_CORE_EVENT_RECORDING_CANCELLED),
            OverlayAction::HideAfter(Duration::from_millis(180))
        );
        assert_eq!(
            overlay_action_for_event(NATIVE_CORE_EVENT_RECOVERED),
            OverlayAction::None
        );
    }
}
