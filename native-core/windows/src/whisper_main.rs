mod whisper_engine;
mod whisper_protocol;

use crate::{
    whisper_engine::WhisperEngine,
    whisper_protocol::{
        WhisperHelperRequest, WhisperHelperResponse, METHOD_TRANSCRIBE, METHOD_WARM,
    },
};
use std::{
    io::{self, BufRead, Write},
    path::Path,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("parrot-whisper failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut engine = WhisperEngine::default();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("failed to read parrot-whisper stdin: {error}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<WhisperHelperRequest>(&line) {
            Ok(request) => handle_request(&mut engine, request),
            Err(error) => WhisperHelperResponse::error("unknown", format!("invalid JSON: {error}")),
        };

        let mut response_line = serde_json::to_string(&response)?;
        response_line.push('\n');
        stdout.write_all(response_line.as_bytes())?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_request(
    engine: &mut WhisperEngine,
    request: WhisperHelperRequest,
) -> WhisperHelperResponse {
    let id = request.id.clone();
    let result = match request.method.as_str() {
        METHOD_WARM => engine
            .warm(Path::new(&request.model_path))
            .map(|_| WhisperHelperResponse::success(id.clone(), None)),
        METHOD_TRANSCRIBE => {
            let Some(audio_path) = request.audio_path.as_deref() else {
                return WhisperHelperResponse::error(
                    id,
                    "transcribe request is missing `audioPath`",
                );
            };
            let Some(language_settings) = request.language_settings.as_ref() else {
                return WhisperHelperResponse::error(
                    id,
                    "transcribe request is missing `languageSettings`",
                );
            };

            engine
                .transcribe_file(
                    Path::new(&request.model_path),
                    Path::new(audio_path),
                    language_settings,
                )
                .map(|transcription| {
                    WhisperHelperResponse::success(id.clone(), Some(transcription))
                })
        }
        method => Err(anyhow::anyhow!("Unknown parrot-whisper method: {method}")),
    };

    match result {
        Ok(response) => response,
        Err(error) => WhisperHelperResponse::error(id, error.to_string()),
    }
}
