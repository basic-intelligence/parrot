# Native Core

Parrot is currently macOS-only, but native-core code is split so future Windows and Linux sidecars can reuse product behavior instead of copying macOS logic.

Shared product logic:
  protocol, settings, model catalog, language routing, prompts,
  cleanup sanitizer, contextual paste formatting, speech trimming,
  model status/download bookkeeping, tests, shared assets

Platform adapters:
  audio capture, input devices, permissions, global shortcuts,
  focused text context, paste injection, sound playback, sidecar launch

## Layout

- `shared/` contains product resources and test fixtures: languages, models, prompts, sound manifests, and behavior fixtures.
- `macos/` contains the implemented Swift sidecar and macOS platform adapters.
- `windows/` is a future sidecar placeholder.
- `linux/` is a future sidecar placeholder.
- `../crates/` contains shared Rust product logic used by the Tauri host and fixture tests.

Future sidecars should keep the same newline-delimited JSON request/response boundary used by the macOS sidecar.

Tauri resource bundling should include shared JSON and prompt resources. Tauri sidecar `externalBin` target-triple naming is reserved for future Windows/Linux sidecars; do not add non-working platform packaging requirements in this foundation refactor.
