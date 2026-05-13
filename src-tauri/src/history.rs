use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub audio_duration_seconds: f64,
    pub raw_transcription: Option<String>,
    pub cleaned_transcription: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryIndex {
    entries: Vec<HistoryEntry>,
}

pub struct HistoryStore {
    index: HistoryIndex,
    path: PathBuf,
}

impl HistoryStore {
    pub fn load(app: &AppHandle) -> anyhow::Result<Self> {
        let dir = app
            .path()
            .app_data_dir()
            .context("missing app data dir")?
            .join("history");
        fs::create_dir_all(&dir)?;
        let path = dir.join("index.json");
        let index = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            HistoryIndex::default()
        };
        Ok(Self { index, path })
    }

    pub fn entries(&self) -> Vec<HistoryEntry> {
        let mut entries = self.index.entries.clone();
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries
    }

    pub fn insert(&mut self, entry: HistoryEntry) -> anyhow::Result<()> {
        self.index.entries.retain(|e| e.id != entry.id);
        self.index.entries.push(entry);
        self.persist()
    }

    pub fn delete(&mut self, id: Uuid) -> anyhow::Result<()> {
        self.index.entries.retain(|e| e.id != id);
        self.persist()
    }

    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.index.entries.clear();
        self.persist()
    }

    fn persist(&self) -> anyhow::Result<()> {
        fs::write(&self.path, serde_json::to_vec_pretty(&self.index)?)?;
        Ok(())
    }
}
