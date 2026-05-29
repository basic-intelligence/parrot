use crate::{
    models::downloads::{model_path, windows_descriptor_for},
    whisper_protocol::{Transcription, WhisperHelperRequest, WhisperHelperResponse},
};
use anyhow::{anyhow, Context};
use parrot_language::{DictationLanguageSettings, SpeechModelSlot};
use parrot_protocol::{AppSettings, NativeCorePaths};
use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
};

#[derive(Clone, Default)]
pub struct WhisperCppPipeline {
    helper: Arc<Mutex<Option<WhisperHelperProcess>>>,
    request_counter: Arc<AtomicU64>,
}

impl WhisperCppPipeline {
    pub fn warm(&self, settings: &AppSettings, paths: &NativeCorePaths) -> anyhow::Result<()> {
        let descriptor = speech_descriptor(settings)?;
        let path = model_path(&descriptor, paths)?;
        ensure_model_file(&path)?;

        let request = WhisperHelperRequest::warm(self.next_request_id(), path_to_string(&path));
        let response = self.request_helper(&request)?;
        ensure_success(response, false).map(|_| ())
    }

    pub fn transcribe(
        &self,
        samples_16khz: &[f32],
        settings: &AppSettings,
        paths: &NativeCorePaths,
    ) -> anyhow::Result<Transcription> {
        self.transcribe_inner(samples_16khz, settings, paths, None)
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub fn transcribe_with_cancel(
        &self,
        samples_16khz: &[f32],
        settings: &AppSettings,
        paths: &NativeCorePaths,
        cancel_flag: Arc<AtomicBool>,
    ) -> anyhow::Result<Transcription> {
        self.transcribe_inner(samples_16khz, settings, paths, Some(cancel_flag))
    }

    fn transcribe_inner(
        &self,
        samples_16khz: &[f32],
        settings: &AppSettings,
        paths: &NativeCorePaths,
        cancel_flag: Option<Arc<AtomicBool>>,
    ) -> anyhow::Result<Transcription> {
        if samples_16khz.is_empty() {
            return Err(anyhow!("No speech detected."));
        }
        if is_cancelled(cancel_flag.as_ref()) {
            return Err(anyhow!("Recording cancelled."));
        }

        let descriptor = speech_descriptor(settings)?;
        let model_path = model_path(&descriptor, paths)?;
        ensure_model_file(&model_path)?;

        let request_id = self.next_request_id();
        let audio_path = write_audio_samples(samples_16khz, paths, &request_id)?;
        let request = WhisperHelperRequest::transcribe(
            request_id,
            path_to_string(&model_path),
            path_to_string(&audio_path),
            DictationLanguageSettings::from(settings),
        );

        let response = if is_cancelled(cancel_flag.as_ref()) {
            Err(anyhow!("Recording cancelled."))
        } else {
            self.request_helper(&request)
        };
        let remove_result = fs::remove_file(&audio_path);
        if let Err(error) = remove_result {
            eprintln!(
                "failed to remove temporary Windows whisper audio {}: {error}",
                audio_path.display()
            );
        }

        if is_cancelled(cancel_flag.as_ref()) {
            return Err(anyhow!("Recording cancelled."));
        }

        ensure_success(response?, true)?
            .ok_or_else(|| anyhow!("parrot-whisper returned no transcription result."))
    }

    fn request_helper(
        &self,
        request: &WhisperHelperRequest,
    ) -> anyhow::Result<WhisperHelperResponse> {
        let mut helper = self.helper.lock().expect("whisper helper poisoned");
        if helper.is_none() {
            *helper = Some(WhisperHelperProcess::start()?);
        }

        let first_result = helper
            .as_mut()
            .expect("whisper helper missing after start")
            .request(request);
        match first_result {
            Ok(response) => Ok(response),
            Err(error) => {
                eprintln!("Windows parrot-whisper request failed; restarting helper: {error}");
                helper.take();
                *helper = Some(WhisperHelperProcess::start()?);
                helper
                    .as_mut()
                    .expect("whisper helper missing after restart")
                    .request(request)
                    .context("parrot-whisper request failed after helper restart")
            }
        }
    }

    fn next_request_id(&self) -> String {
        let id = self.request_counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("whisper-{id}")
    }
}

struct WhisperHelperProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl WhisperHelperProcess {
    fn start() -> anyhow::Result<Self> {
        let helper_path = helper_executable_path()?;
        if !helper_path.exists() {
            return Err(anyhow!(
                "parrot-whisper helper is missing: {}",
                helper_path.display()
            ));
        }

        let mut command = Command::new(&helper_path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn {}", helper_path.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open parrot-whisper stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to open parrot-whisper stdout"))?;
        if let Some(stderr) = child.stderr.take() {
            drain_stderr(stderr);
        }

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn request(&mut self, request: &WhisperHelperRequest) -> anyhow::Result<WhisperHelperResponse> {
        let line = serde_json::to_string(request)? + "\n";
        self.stdin
            .write_all(line.as_bytes())
            .context("failed to write parrot-whisper request")?;
        self.stdin
            .flush()
            .context("failed to flush parrot-whisper request")?;

        let mut response_line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut response_line)
            .context("failed to read parrot-whisper response")?;
        if bytes == 0 {
            return Err(anyhow!("parrot-whisper helper closed stdout"));
        }

        let response: WhisperHelperResponse = serde_json::from_str(response_line.trim())
            .context("invalid parrot-whisper response")?;
        if response.id != request.id {
            return Err(anyhow!(
                "parrot-whisper returned response id `{}` for request `{}`",
                response.id,
                request.id
            ));
        }

        Ok(response)
    }
}

impl Drop for WhisperHelperProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn ensure_success(
    response: WhisperHelperResponse,
    expects_result: bool,
) -> anyhow::Result<Option<Transcription>> {
    if response.ok {
        return Ok(response.result);
    }

    let fallback = if expects_result {
        "parrot-whisper transcription failed without an error message."
    } else {
        "parrot-whisper warmup failed without an error message."
    };
    Err(anyhow!(response.error.unwrap_or_else(|| fallback.into())))
}

fn speech_descriptor(settings: &AppSettings) -> anyhow::Result<parrot_models::ModelDescriptor> {
    let language_settings = DictationLanguageSettings::from(settings);
    let public_id = match parrot_language::speech_model_slot(&language_settings) {
        SpeechModelSlot::Speech => "speech",
        SpeechModelSlot::SpeechMultilingual => "speech-multilingual",
    };
    windows_descriptor_for(public_id)
        .ok_or_else(|| anyhow!("No Windows speech model is available for {public_id}."))
}

fn write_audio_samples(
    samples_16khz: &[f32],
    paths: &NativeCorePaths,
    request_id: &str,
) -> anyhow::Result<PathBuf> {
    let temp_dir = if paths.temp_dir.trim().is_empty() {
        env::temp_dir()
    } else {
        PathBuf::from(&paths.temp_dir)
    };
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create temp directory {}", temp_dir.display()))?;

    let audio_path = temp_dir.join(format!(
        "parrot-whisper-{}-{request_id}.f32",
        std::process::id()
    ));
    let mut bytes = Vec::with_capacity(samples_16khz.len() * std::mem::size_of::<f32>());
    for sample in samples_16khz {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    fs::write(&audio_path, bytes)
        .with_context(|| format!("failed to write {}", audio_path.display()))?;
    Ok(audio_path)
}

fn helper_executable_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("PARROT_WHISPER_HELPER_PATH") {
        return Ok(PathBuf::from(path));
    }

    let current_exe = env::current_exe().context("failed to locate parrot-core executable")?;
    let current_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("parrot-core.exe");
    let helper_name = helper_name_for_core_name(current_name);
    Ok(current_exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(helper_name))
}

fn helper_name_for_core_name(core_name: &str) -> String {
    core_name
        .strip_prefix("parrot-core")
        .map(|suffix| format!("parrot-whisper{suffix}"))
        .unwrap_or_else(|| {
            if core_name.ends_with(".exe") {
                "parrot-whisper.exe".into()
            } else {
                "parrot-whisper".into()
            }
        })
}

fn drain_stderr(stderr: std::process::ChildStderr) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.trim().is_empty() => eprintln!("parrot-whisper stderr: {line}"),
                Ok(_) => {}
                Err(error) => {
                    eprintln!("failed to read parrot-whisper stderr: {error}");
                    break;
                }
            }
        }
    });
}

fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}

fn ensure_model_file(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(anyhow!("Model download required: {}", path.display()))
    }
}

fn is_cancelled(cancel_flag: Option<&Arc<AtomicBool>>) -> bool {
    cancel_flag
        .map(|flag| flag.load(Ordering::SeqCst))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_name_preserves_tauri_variant_suffix() {
        assert_eq!(
            helper_name_for_core_name("parrot-core-cpu-x86_64-pc-windows-msvc.exe"),
            "parrot-whisper-cpu-x86_64-pc-windows-msvc.exe"
        );
        assert_eq!(
            helper_name_for_core_name("parrot-core-cuda-x86_64-pc-windows-msvc.exe"),
            "parrot-whisper-cuda-x86_64-pc-windows-msvc.exe"
        );
    }

    #[test]
    fn helper_name_falls_back_for_unexpected_core_name() {
        assert_eq!(
            helper_name_for_core_name("parrot-core.exe"),
            "parrot-whisper.exe"
        );
        assert_eq!(helper_name_for_core_name("unit-test"), "parrot-whisper");
    }
}
