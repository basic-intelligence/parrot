mod json_lines;
mod model_downloads;
mod model_llama_cpp;
mod model_whisper_cpp;
mod platform;
mod service;

use crate::{
    json_lines::{error_response, parse_request_line, serialize_json_line},
    service::LinuxNativeService,
};
use serde_json::Value;
use std::{
    io::{self, BufRead, Write},
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let stdin = io::stdin();
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Value>();
    let output_stdout = Arc::clone(&stdout);
    std::thread::spawn(move || {
        while let Some(value) = output_rx.blocking_recv() {
            if let Err(error) = write_json_line(&output_stdout, &value) {
                eprintln!("failed to write native-core output: {error}");
                break;
            }
        }
    });

    let mut service = LinuxNativeService::new(output_tx.clone());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("failed to read native-core stdin: {error}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let response = match parse_request_line(&line) {
            Ok(request) => service.handle_request(request).await,
            Err(error) => {
                eprintln!("invalid native-core request line: {}", error.message);
                error_response(error.id.as_deref().unwrap_or("unknown"), error.message)
            }
        };

        if output_tx.send(response).is_err() {
            break;
        }
    }
}

fn write_json_line(stdout: &Arc<Mutex<io::Stdout>>, value: &Value) -> anyhow::Result<()> {
    let response_line = serialize_json_line(value)?;
    let mut stdout = stdout.lock().expect("native-core stdout poisoned");
    stdout.write_all(response_line.as_bytes())?;
    stdout.flush()?;
    Ok(())
}
