use anyhow::{anyhow, Context};
use parrot_language::DictationLanguageSettings;
use parrot_models::{
    catalog, model_for_public_id, model_status, required_models, Architecture, ModelDescriptor,
    ModelFileState, Platform,
};
use parrot_protocol::{AppSettings, ModelRole, ModelStatus, NativeCorePaths};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const DOWNLOAD_CHUNK_SIZE: usize = 1024 * 1024;
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
pub struct WindowsModelStore {
    paths: Arc<Mutex<Option<NativeCorePaths>>>,
    downloads: Arc<Mutex<HashMap<String, DownloadProgress>>>,
}

#[derive(Debug, Clone)]
struct DownloadProgress {
    downloading: bool,
    progress_bytes: i64,
    progress_total_bytes: i64,
    error: Option<String>,
}

impl WindowsModelStore {
    pub fn configure_paths(&self, paths: NativeCorePaths) {
        *self.paths.lock().expect("model paths poisoned") = Some(paths);
    }

    pub fn statuses(&self, settings: &AppSettings) -> anyhow::Result<Vec<ModelStatus>> {
        let required_ids = required_models(
            &DictationLanguageSettings::from(settings),
            Platform::Windows,
            Architecture::X86_64,
        )
        .into_iter()
        .map(|model| model.public_id)
        .collect::<HashSet<_>>();

        let paths = self.paths.lock().expect("model paths poisoned").clone();
        let mut statuses = Vec::new();
        for descriptor in windows_descriptors() {
            let state = model_file_state(&descriptor, paths.as_ref(), &self.downloads);
            statuses.push(model_status(
                &descriptor,
                state,
                required_ids.contains(&descriptor.public_id),
            ));
        }
        Ok(statuses)
    }

    pub fn start_download(
        &self,
        public_id: &str,
        settings: &AppSettings,
        on_success: Option<Box<dyn FnOnce() + Send + 'static>>,
    ) -> anyhow::Result<Vec<ModelStatus>> {
        let descriptor = windows_descriptor_for(public_id)
            .ok_or_else(|| anyhow!("Unknown Windows model: {public_id}"))?;
        let paths = self
            .paths
            .lock()
            .expect("model paths poisoned")
            .clone()
            .ok_or_else(|| anyhow!("native-core paths are not initialized"))?;
        let model_path = model_path(&descriptor, &paths)?;
        if model_path.exists() {
            return self.statuses(settings);
        }

        {
            let mut downloads = self.downloads.lock().expect("model downloads poisoned");
            if downloads
                .get(&descriptor.public_id)
                .map(|progress| progress.downloading)
                .unwrap_or(false)
            {
                return self.statuses(settings);
            }
            downloads.insert(
                descriptor.public_id.clone(),
                DownloadProgress {
                    downloading: true,
                    progress_bytes: existing_temp_bytes(&descriptor, &paths),
                    progress_total_bytes: descriptor.expected_bytes.max(1),
                    error: None,
                },
            );
        }

        let downloads = Arc::clone(&self.downloads);
        thread::spawn(move || {
            let result = download_descriptor(&descriptor, &paths, Arc::clone(&downloads));
            let callback = {
                let mut downloads = downloads.lock().expect("model downloads poisoned");
                match result {
                    Ok(()) => {
                        downloads.remove(&descriptor.public_id);
                        on_success
                    }
                    Err(error) => {
                        downloads.insert(
                            descriptor.public_id.clone(),
                            DownloadProgress {
                                downloading: false,
                                progress_bytes: existing_temp_bytes(&descriptor, &paths),
                                progress_total_bytes: descriptor.expected_bytes.max(1),
                                error: Some(error.to_string()),
                            },
                        );
                        None
                    }
                }
            };
            if let Some(on_success) = callback {
                on_success();
            }
        });

        self.statuses(settings)
    }

    pub fn delete_model(
        &self,
        public_id: &str,
        settings: &AppSettings,
    ) -> anyhow::Result<Vec<ModelStatus>> {
        let descriptor = windows_descriptor_for(public_id)
            .ok_or_else(|| anyhow!("Unknown Windows model: {public_id}"))?;
        let paths = self
            .paths
            .lock()
            .expect("model paths poisoned")
            .clone()
            .ok_or_else(|| anyhow!("native-core paths are not initialized"))?;
        remove_if_exists(model_path(&descriptor, &paths)?)?;
        remove_if_exists(temp_model_path(&descriptor, &paths)?)?;
        self.downloads
            .lock()
            .expect("model downloads poisoned")
            .remove(public_id);
        self.statuses(settings)
    }
}

pub fn windows_descriptor_for(public_id: &str) -> Option<ModelDescriptor> {
    model_for_public_id(public_id, Platform::Windows, Architecture::X86_64)
}

pub fn model_path(
    descriptor: &ModelDescriptor,
    paths: &NativeCorePaths,
) -> anyhow::Result<PathBuf> {
    let file_name = descriptor
        .file_name
        .as_deref()
        .ok_or_else(|| anyhow!("model `{}` is missing a file name", descriptor.public_id))?;
    Ok(model_dir(descriptor, paths).join(file_name))
}

fn windows_descriptors() -> Vec<ModelDescriptor> {
    catalog()
        .models
        .into_iter()
        .filter(|model| model.platforms.contains(&Platform::Windows))
        .filter(|model| model.architectures.contains(&Architecture::X86_64))
        .collect()
}

fn model_file_state(
    descriptor: &ModelDescriptor,
    paths: Option<&NativeCorePaths>,
    downloads: &Arc<Mutex<HashMap<String, DownloadProgress>>>,
) -> ModelFileState {
    if let Some(progress) = downloads
        .lock()
        .expect("model downloads poisoned")
        .get(&descriptor.public_id)
        .cloned()
    {
        return ModelFileState {
            local_bytes: local_model_bytes(descriptor, paths),
            downloading: progress.downloading,
            progress_bytes: progress.progress_bytes,
            progress_total_bytes: progress.progress_total_bytes,
            error: progress.error,
        };
    }

    let local_bytes = local_model_bytes(descriptor, paths);
    let progress_bytes = if local_bytes > 0 {
        local_bytes
    } else {
        paths
            .map(|paths| existing_temp_bytes(descriptor, paths))
            .unwrap_or(0)
    };

    ModelFileState {
        local_bytes,
        downloading: false,
        progress_bytes,
        progress_total_bytes: descriptor.expected_bytes.max(progress_bytes).max(1),
        error: None,
    }
}

fn local_model_bytes(descriptor: &ModelDescriptor, paths: Option<&NativeCorePaths>) -> i64 {
    paths
        .and_then(|paths| model_path(descriptor, paths).ok())
        .and_then(|path| path.metadata().ok())
        .map(|metadata| metadata.len().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn existing_temp_bytes(descriptor: &ModelDescriptor, paths: &NativeCorePaths) -> i64 {
    temp_model_path(descriptor, paths)
        .ok()
        .and_then(|path| path.metadata().ok())
        .map(|metadata| metadata.len().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn download_descriptor(
    descriptor: &ModelDescriptor,
    paths: &NativeCorePaths,
    downloads: Arc<Mutex<HashMap<String, DownloadProgress>>>,
) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create download runtime")?
        .block_on(download_descriptor_async(descriptor, paths, downloads))
}

async fn download_descriptor_async(
    descriptor: &ModelDescriptor,
    paths: &NativeCorePaths,
    downloads: Arc<Mutex<HashMap<String, DownloadProgress>>>,
) -> anyhow::Result<()> {
    let output_path = model_path(descriptor, paths)?;
    let temp_path = temp_model_path(descriptor, paths)?;
    fs::create_dir_all(
        output_path
            .parent()
            .ok_or_else(|| anyhow!("model output path has no parent"))?,
    )?;
    remove_if_exists(&temp_path)?;

    let url = download_url(descriptor)?;
    let client = reqwest::Client::builder()
        .user_agent("Parrot/0.2.0 Windows")
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .build()
        .context("failed to create download client")?;
    let mut response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("model download failed: {url}"))?;
    let total_bytes = response
        .content_length()
        .map(|value| value.min(i64::MAX as u64) as i64)
        .unwrap_or(descriptor.expected_bytes)
        .max(1);
    update_download_progress(&downloads, descriptor, 0, total_bytes);

    let mut file = File::create(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    let mut downloaded = 0_i64;
    loop {
        let chunk = response
            .chunk()
            .await
            .with_context(|| format!("failed to read {url}"))?;
        let Some(chunk) = chunk else { break };

        file.write_all(&chunk)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        downloaded = downloaded.saturating_add(chunk.len() as i64);
        update_download_progress(&downloads, descriptor, downloaded, total_bytes);
    }
    file.flush()
        .with_context(|| format!("failed to flush {}", temp_path.display()))?;

    if let Some(expected_hash) = descriptor.sha256.as_deref() {
        let actual_hash = sha256_file(&temp_path)?;
        if !actual_hash.eq_ignore_ascii_case(expected_hash) {
            return Err(anyhow!(
                "{} checksum mismatch: expected {}, got {}",
                descriptor.public_id,
                expected_hash,
                actual_hash
            ));
        }
    }

    fs::rename(&temp_path, &output_path).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp_path.display(),
            output_path.display()
        )
    })?;
    Ok(())
}

fn update_download_progress(
    downloads: &Arc<Mutex<HashMap<String, DownloadProgress>>>,
    descriptor: &ModelDescriptor,
    downloaded: i64,
    total: i64,
) {
    downloads.lock().expect("model downloads poisoned").insert(
        descriptor.public_id.clone(),
        DownloadProgress {
            downloading: true,
            progress_bytes: downloaded,
            progress_total_bytes: total.max(1),
            error: None,
        },
    );
}

fn download_url(descriptor: &ModelDescriptor) -> anyhow::Result<String> {
    let repo_id = descriptor
        .repo_id
        .as_deref()
        .ok_or_else(|| anyhow!("model `{}` is missing a repo", descriptor.public_id))?;
    let file_name = descriptor
        .file_name
        .as_deref()
        .ok_or_else(|| anyhow!("model `{}` is missing a file name", descriptor.public_id))?;
    Ok(format!(
        "https://huggingface.co/{repo_id}/resolve/main/{file_name}?download=true"
    ))
}

fn model_dir(descriptor: &ModelDescriptor, paths: &NativeCorePaths) -> PathBuf {
    match descriptor.role {
        ModelRole::Speech => PathBuf::from(&paths.speech_models_dir),
        ModelRole::Cleanup => PathBuf::from(&paths.cleanup_models_dir),
    }
}

fn temp_model_path(
    descriptor: &ModelDescriptor,
    paths: &NativeCorePaths,
) -> anyhow::Result<PathBuf> {
    let file_name = descriptor
        .file_name
        .as_deref()
        .ok_or_else(|| anyhow!("model `{}` is missing a file name", descriptor.public_id))?;
    Ok(model_dir(descriptor, paths).join(format!("{file_name}.download")))
}

fn remove_if_exists(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; DOWNLOAD_CHUNK_SIZE];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
