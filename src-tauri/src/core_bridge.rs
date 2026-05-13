use anyhow::{anyhow, Context};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{unix::OwnedWriteHalf, UnixListener},
    process::Command,
    sync::{oneshot, Mutex},
};
use uuid::Uuid;

struct PendingRequest {
    generation: u64,
    tx: oneshot::Sender<anyhow::Result<Value>>,
}

#[derive(Clone)]
pub struct CoreBridge {
    app: AppHandle,
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    pending: Arc<Mutex<HashMap<String, PendingRequest>>>,
    generation: Arc<AtomicU64>,
    reconnect_lock: Arc<Mutex<()>>,
}

impl CoreBridge {
    pub async fn spawn(app: AppHandle) -> anyhow::Result<Self> {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let writer = Arc::new(Mutex::new(None));
        let generation = Arc::new(AtomicU64::new(0));
        let write_half = Self::connect_socket(
            app.clone(),
            writer.clone(),
            pending.clone(),
            generation.clone(),
        )
        .await?;

        *writer.lock().await = Some(write_half);

        Ok(Self {
            app,
            writer,
            pending,
            generation,
            reconnect_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn reconnect(&self) -> anyhow::Result<()> {
        let _guard = self.reconnect_lock.lock().await;

        fail_all_pending(&self.pending, "native core reconnecting").await;

        let write_half = Self::connect_socket(
            self.app.clone(),
            self.writer.clone(),
            self.pending.clone(),
            self.generation.clone(),
        )
        .await?;

        *self.writer.lock().await = Some(write_half);

        let _ = self.app.emit(
            "parrot:native-core-recovered",
            json!({ "status": "Parrot Core reconnected." }),
        );

        Ok(())
    }

    async fn connect_socket(
        app: AppHandle,
        writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
        pending: Arc<Mutex<HashMap<String, PendingRequest>>>,
        generation: Arc<AtomicU64>,
    ) -> anyhow::Result<OwnedWriteHalf> {
        let connection_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let app_bundle = core_app_bundle_path(&app)?;
        let socket_path = std::env::temp_dir().join(format!("parrot-core-{}.sock", Uuid::new_v4()));
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).with_context(|| {
            format!(
                "failed to bind native core socket at {}",
                socket_path.display()
            )
        })?;

        let socket_arg = socket_path
            .to_str()
            .context("native core socket path is not valid UTF-8")?
            .to_string();

        launch_native_core(&app_bundle, &socket_arg).await?;

        let accept = tokio::time::timeout(Duration::from_secs(20), listener.accept()).await;
        let (stream, _) = match accept {
            Ok(Ok(pair)) => pair,
            Ok(Err(error)) => {
                let _ = std::fs::remove_file(&socket_path);
                return Err(error).context("native core socket accept failed");
            }
            Err(_) => {
                let _ = std::fs::remove_file(&socket_path);
                return Err(anyhow!(
                    "native core did not connect back to socket after launching {}",
                    app_bundle.display()
                ));
            }
        };

        let _ = std::fs::remove_file(&socket_path);

        let (read_half, write_half) = stream.into_split();

        let app_for_events = app.clone();
        let writer_for_close = writer.clone();
        let generation_for_close = generation.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(read_half).lines();
            let close_message: String;
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim();
                        if !line.is_empty() {
                            process_stdout_line(&app_for_events, &pending, line).await;
                        }
                    }
                    Ok(None) => {
                        close_message = "native-core socket closed".to_string();
                        eprintln!("{close_message}");
                        break;
                    }
                    Err(error) => {
                        close_message = format!("native-core socket read error: {error}");
                        eprintln!("{close_message}");
                        break;
                    }
                }
            }

            fail_pending_generation(&pending, connection_generation, close_message.clone()).await;

            if generation_for_close.load(Ordering::SeqCst) == connection_generation {
                *writer_for_close.lock().await = None;
                hide_recording_overlay(&app_for_events);
                let _ = app_for_events.emit(
                    "parrot:native-core-disconnected",
                    json!({ "error": close_message }),
                );
            }
        });

        Ok(write_half)
    }

    pub async fn request(&self, method: &str, payload: Value) -> anyhow::Result<Value> {
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

        let line = json!({ "id": id, "method": method, "payload": payload }).to_string() + "\n";
        let write_result = {
            let mut writer = self.writer.lock().await;
            match writer.as_mut() {
                Some(writer) => Some(writer.write_all(line.as_bytes()).await),
                None => None,
            }
        };

        match write_result {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                self.pending.lock().await.remove(&id);
                return Err(error)
                    .with_context(|| format!("failed to write native core request `{method}`"));
            }
            None => {
                self.pending.lock().await.remove(&id);
                return Err(anyhow!("native core socket is not connected"));
            }
        }

        match tokio::time::timeout(Duration::from_secs(300), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow!("native core response channel closed")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(anyhow!("native core timed out on {method}"))
            }
        }
    }
}

async fn launch_native_core(app_bundle: &PathBuf, socket_arg: &str) -> anyhow::Result<()> {
    if std::env::var_os("PARROT_CORE_DIRECT_LAUNCH").is_some() {
        let executable = app_bundle
            .join("Contents")
            .join("MacOS")
            .join("parrot-core");
        let mut child = Command::new(&executable)
            .arg("--socket")
            .arg(socket_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to launch native core executable at {}",
                    executable.display()
                )
            })?;

        if let Some(stderr) = child.stderr.take() {
            tauri::async_runtime::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("parrot-core stderr: {line}");
                }
            });
        }

        tauri::async_runtime::spawn(async move {
            match child.wait().await {
                Ok(status) => eprintln!("parrot-core direct process exited with {status}"),
                Err(error) => eprintln!("parrot-core direct process wait failed: {error}"),
            }
        });

        return Ok(());
    }

    let status = Command::new("/usr/bin/open")
        .arg("-n")
        .arg(app_bundle)
        .arg("--args")
        .arg("--socket")
        .arg(socket_arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .with_context(|| {
            format!(
                "failed to launch native core app at {}",
                app_bundle.display()
            )
        })?;

    if !status.success() {
        return Err(anyhow!(
            "failed to launch native core app at {}; open exited with {status}",
            app_bundle.display()
        ));
    }

    Ok(())
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
        || message.contains("socket closed")
        || message.contains("not connected")
        || message.contains("connection reset")
        || message.contains("connection aborted")
        || message.contains("response channel closed")
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

fn core_app_bundle_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let mut candidates = Vec::new();

        if let Some(contents_dir) = resource_dir.parent() {
            candidates.push(contents_dir.join("Helpers").join("Parrot.app"));
        }

        // Legacy / dev fallback locations.
        candidates.push(resource_dir.join("binaries").join("Parrot.app"));
        candidates.push(resource_dir.join("Parrot.app"));

        for candidate in candidates {
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    let cwd = std::env::current_dir()?;
    for base in [cwd.clone(), cwd.join("src-tauri")] {
        let candidate = base.join("binaries").join("Parrot.app");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!("failed to locate Parrot.app native helper"))
}

async fn process_stdout_line(
    app: &AppHandle,
    pending: &Arc<Mutex<HashMap<String, PendingRequest>>>,
    line: &str,
) {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("native-core invalid json: {error}; line={line}");
            return;
        }
    };

    if let Some(event_name) = parsed.get("event").and_then(|v| v.as_str()) {
        let payload = parsed.get("payload").cloned().unwrap_or(Value::Null);
        handle_native_event(app, event_name, payload);
        return;
    }

    let Some(id) = parsed
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        eprintln!("native-core response missing id: {parsed}");
        return;
    };
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
    if let Some(request) = pending.lock().await.remove(&id) {
        let _ = request.tx.send(result);
    }
}

fn handle_native_event(app: &AppHandle, event_name: &str, payload: Value) {
    match event_name {
        "parrot:recording-started" => show_recording_overlay(app),
        "parrot:recording-processing" => show_recording_overlay(app),
        "parrot:recording-finished" => {
            hide_recording_overlay_after(app.clone(), Duration::from_millis(120))
        }
        "parrot:recording-failed" => {
            hide_recording_overlay_after(app.clone(), Duration::from_millis(700))
        }
        "parrot:recording-cancelled" => {
            hide_recording_overlay_after(app.clone(), Duration::from_millis(180))
        }
        _ => {}
    }
    let _ = app.emit(event_name, payload);
}

fn show_recording_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window("recording") else {
        return;
    };
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        let width = window.outer_size().map(|s| s.width as i32).unwrap_or(148);
        let height = window.outer_size().map(|s| s.height as i32).unwrap_or(36);
        let x = monitor_pos.x + ((monitor_size.width as i32 - width) / 2);
        let y = monitor_pos.y + monitor_size.height as i32 - height - 96;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
}

fn hide_recording_overlay(app: &AppHandle) {
    let Some(window) = app.get_webview_window("recording") else {
        return;
    };
    let _ = window.hide();
}

fn hide_recording_overlay_after(app: AppHandle, delay: Duration) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        hide_recording_overlay(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_native_core_disconnect_errors() {
        let broken_pipe = anyhow!(std::io::Error::from(ErrorKind::BrokenPipe))
            .context("failed to write native core request `captureShortcut`");
        assert!(is_native_core_disconnect(&broken_pipe));

        let socket_closed = anyhow!("native-core socket closed");
        assert!(is_native_core_disconnect(&socket_closed));

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

            fail_pending_generation(&pending, 1, "socket closed").await;

            assert!(rx_one.await.unwrap().is_err());
            assert!(pending.lock().await.contains_key("two"));

            fail_all_pending(&pending, "native core reconnecting").await;
            assert!(rx_two.await.unwrap().is_err());
            assert!(pending.lock().await.is_empty());
        });
    }
}
