# Parrot Native Core for Windows

The Windows native core is a Rust sidecar named `parrot-core`. The Tauri host launches it as an `externalBin` sidecar and communicates over newline-delimited JSON on stdin/stdout. Logs and diagnostics go to stderr so stdout stays protocol-only.

Windows is implemented and built as signed CPU and CUDA sidecar variants. The host selects the appropriate `parrot-core` sidecar at runtime, and each core sidecar starts the matching `parrot-whisper` helper for speech transcription.

## Architecture

- `src/main.rs` reads JSON request lines and writes JSON responses/events.
- `src/whisper_main.rs` runs the private `parrot-whisper` helper protocol.
- `src/service.rs` owns Windows-side orchestration.
- `src/json_lines.rs` contains protocol line parsing and serialization.
- `src/platform/` contains Windows adapters for audio, permissions, hotkeys, shortcut capture, focused text, paste, and sound.
- `src/models/` contains model download/status/delete logic and local runtime integration.
- `../shared/` contains shared model catalog, prompts, language data, sounds, and fixtures.

## Build

From the repository root:

```powershell
npm run build:core:windows
```

For an unsigned local QA installer:

```powershell
npm run build:windows:qa
```

The sidecar build writes variant binaries under:

```text
src-tauri/binaries/parrot-core-<variant>-x86_64-pc-windows-msvc.exe
src-tauri/binaries/parrot-whisper-<variant>-x86_64-pc-windows-msvc.exe
```

## Tests

```powershell
cargo test --manifest-path native-core/windows/Cargo.toml --no-default-features --features core-bin --bin parrot-core
cargo test --manifest-path native-core/windows/Cargo.toml --no-default-features --features whisper-bin --bin parrot-whisper
```

The repo CI also runs the Rust workspace tests, frontend build, and Windows sidecar build.

## Platform notes

- Windows requires microphone access.
- Hotkeys use a low-level keyboard hook.
- Focused text context uses UI Automation when available.
- Paste uses the clipboard plus `SendInput`.
- Paste into elevated/admin apps can fail because normal user apps cannot inject input into higher-integrity apps.
- Fn is not a reliable Windows shortcut key, so Windows defaults use Windows-visible keys.
- Speech models use whisper.cpp GGML files through the separate `parrot-whisper` process and `whisper-rs`, cleanup models use GGUF files through `llama_cpp` in `parrot-core`, and runtime model files are stored in the non-roaming local model cache provided by the Tauri host.
- Keep product behavior in shared crates/resources and OS behavior in Windows adapters.
