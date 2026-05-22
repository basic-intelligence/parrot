# Parrot Native Core for macOS

The macOS native core is a Swift sidecar. The Tauri host starts it as the bundled Parrot Core helper and communicates over newline-delimited JSON.

The sidecar owns macOS-specific behavior: audio capture, input devices, permissions, global shortcuts, shortcut capture, focused text context, paste, sound playback, local speech-to-text, and local cleanup.

## Architecture

- `Sources/ParrotCore/` contains the main sidecar service and macOS platform adapters.
- `Sources/ParrotWhisper/` contains the persistent whisper.cpp helper used by Intel/AMD speech models.
- `Sources/WhisperCppBridge/` wraps whisper.cpp for Swift.
- `Tests/ParrotCoreTests/` contains macOS sidecar tests.
- `../shared/` contains shared model catalog, prompts, language data, sounds, and fixtures. Do not duplicate those values in macOS-only code.

## Build

From the repository root:

```sh
npm run build:core:mac
```

The full Tauri dev/build commands call this automatically on macOS.

## Tests

```sh
swift test --package-path native-core/macos
```

## Platform notes

- Apple Silicon speech models use WhisperKit.
- Intel/AMD speech models use whisper.cpp.
- Cleanup uses local llama.cpp/GGUF models.
- macOS permissions are handled by the sidecar and surfaced through the Tauri app.
- Keep the newline-delimited JSON protocol stable across platform sidecars.
