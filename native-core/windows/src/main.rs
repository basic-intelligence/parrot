mod json_lines;
mod models;
mod platform;
mod service;

use crate::{
    json_lines::{error_response, parse_request_line, serialize_json_line},
    service::CoreService,
};
use serde_json::Value;
use std::{
    io::{self, BufRead, Write},
    sync::{mpsc, Arc, Mutex},
};

fn main() {
    let stdin = io::stdin();
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let (event_tx, event_rx) = mpsc::channel::<Value>();
    let event_stdout = Arc::clone(&stdout);
    std::thread::spawn(move || {
        for event in event_rx {
            if let Err(error) = write_json_line(&event_stdout, &event) {
                eprintln!("failed to write native-core event: {error}");
                break;
            }
        }
    });

    let mut service = CoreService::with_event_sender(event_tx);

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
            Ok(request) => service.handle_request(request),
            Err(error) => {
                eprintln!("invalid native-core request line: {}", error.message);
                error_response(error.id.as_deref().unwrap_or("unknown"), error.message)
            }
        };

        if let Err(error) = write_json_line(&stdout, &response) {
            eprintln!("failed to write native-core stdout: {error}");
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
