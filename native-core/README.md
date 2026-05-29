# Native Core

Parrot native-core code keeps shared product behavior in reusable crates and resources while platform adapters live in per-OS sidecars. macOS and Windows are implemented; Linux is implemented as a first-pass x86_64 CPU-only Rust sidecar.

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
- `windows/` contains the implemented Rust sidecar and Windows platform adapters.
- `linux/` contains the first-pass Rust sidecar and Linux platform adapters.
- `../crates/` contains shared Rust product logic used by the Tauri host and fixture tests.

## Platform READMEs

- [macOS](macos/README.md)
- [Windows](windows/README.md)
- [Linux](linux/README.md)

Sidecars keep the same newline-delimited JSON request/response boundary.

Tauri resource bundling includes shared JSON and prompt resources. Platform-specific build, runtime, signing, and behavior notes live in each platform README.
