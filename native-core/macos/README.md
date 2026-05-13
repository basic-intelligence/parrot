# macOS Sidecar

This Swift package implements Parrot's current native sidecar for macOS. It owns audio capture, global hotkey monitoring, local WhisperKit and whisper.cpp transcription, local llama.cpp/Qwen cleanup, paste integration, and macOS permission checks.

The sidecar protocol is newline-delimited JSON. In the bundled app, the Tauri host starts the helper app and passes a Unix domain socket path with `--socket`; direct stdin/stdout mode remains available for development. Keep this request/response boundary stable so future platform sidecars can implement the same behavior.
