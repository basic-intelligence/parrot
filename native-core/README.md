# Native Core

`native-core/shared` contains shared cleanup prompts and the shared dictation language catalog. The Tauri host passes shared behavior, including cleanup prompts, language catalog data, and current settings, to whichever native sidecar is running.

`native-core/macos` is the only implemented native sidecar today. It is macOS-specific Swift code that owns audio capture, global hotkey monitoring, local Whisper transcription, local llama.cpp/Qwen cleanup, permissions, and paste integration.

`native-core/windows` and `native-core/linux` are placeholders for future sidecars; they are not supported targets today.

Future sidecars should keep the same newline-delimited JSON request/response boundary used by the macOS sidecar.
