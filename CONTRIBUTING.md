# Contributing to Parrot

Thanks for wanting to help make Parrot better.

Parrot is a local-first, open-source dictation app. Contributions are welcome across code, design, docs, testing, packaging, accessibility, privacy review, language support, and platform support.

## Ways to contribute

Helpful contributions include:

- Bug reports with clear reproduction steps.
- Feature requests that explain the user problem.
- Pull requests that fix bugs or improve the app.
- Testing on different Macs, Windows PCs, microphones, languages, and OS versions.
- Documentation improvements.
- Privacy, security, and accessibility feedback.
- Windows QA, packaging/signing testing, and Linux sidecar work.

## Before opening an issue

Please check existing issues first to avoid duplicates.

When reporting a bug, include:

- Parrot version.
- Operating system version.
- Hardware and CPU architecture.
- What you expected to happen.
- What actually happened.
- Steps to reproduce.
- Screenshots or logs, if helpful.

Please do not include private transcripts, recordings, names, API keys, secrets, or sensitive information in public issues.

For security or privacy vulnerabilities, do not open a public issue. See [SECURITY.md](SECURITY.md).

## Development setup

Parrot is a Tauri 2 desktop app with a TypeScript/Vite frontend, Rust host, Swift macOS native sidecar, and Rust Windows sidecar.

Install dependencies:

```sh
npm ci
```

Run the full app in development:

```sh
npm run dev
```

Run only the frontend:

```sh
npm run dev:ui
```

Build the frontend:

```sh
npm run build:ui
```

Build the macOS native sidecar:

```sh
npm run build:core:mac
```

Build the Windows native sidecar on Windows:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-native-core-windows.ps1
```

The npm wrapper is:

```sh
npm run build:core:windows
```

Run Rust tests:

```sh
cargo test --manifest-path src-tauri/Cargo.toml
```

Run Swift tests:

```sh
swift test --package-path native-core/macos
```

Run Windows sidecar tests:

```sh
cargo test -p parrot-core --manifest-path native-core/windows/Cargo.toml
```

Build a packaged app:

```sh
npm run build
```

## Windows development setup

Windows development uses the MSVC Rust target and native C/C++ build tools:

- Windows 10 or Windows 11.
- Visual Studio Build Tools with Desktop development with C++.
- Rust stable with `x86_64-pc-windows-msvc`.
- Node.js LTS and npm.
- CMake.
- Ninja when native model runtime dependencies require it.

Install the Rust target if needed:

```powershell
rustup target add x86_64-pc-windows-msvc
```

The PowerShell build script compiles `native-core/windows`, verifies the sidecar binary, signs it when signing variables are present, and copies it to the Tauri sidecar path under `src-tauri/binaries/`.

For local Windows QA without release signing or updater signing artifacts, run:

```powershell
npm run build:windows:qa
```

The installer will be under `target\x86_64-pc-windows-msvc\release\bundle\nsis\`.

Windows release builds produce signed CPU and CUDA sidecar variants. The host selects the appropriate Windows sidecar at runtime, and whisper.cpp and llama.cpp runtime work must continue to build cleanly under MSVC.

For Windows sidecar architecture and platform behavior notes, see `native-core/windows/README.md`.

## Windows release signing

Windows release builds use a certificate + SignTool Authenticode flow, separate from the Tauri updater signing key. Release CI imports the PFX from `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD`, signs the Tauri executable, Windows sidecar, bundled DLLs when present, and NSIS installer, then runs:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify-windows-signatures.ps1
```

Before publishing a Windows release, confirm `Get-AuthenticodeSignature` reports `Valid` for the app executable, `parrot-core.exe`, and the installer. Keep certificates, private keys, passwords, signing configs, and signing logs in CI secrets or transient CI files only. SmartScreen can still require publisher/file reputation even when Authenticode signatures are valid.

## Project structure

- `src/` — TypeScript frontend.
- `src-tauri/src/` — Rust/Tauri host.
- `native-core/macos/` — Swift macOS sidecar.
- `native-core/windows/` — Rust Windows sidecar and platform adapters.
- `native-core/shared/` — Shared product resources and test fixtures.
- `crates/` — Shared Rust product logic and DTOs.
- `public/` — Static frontend assets.
- `src-tauri/icons/` — App and tray icons.
- `src-tauri/capabilities/` — Tauri permissions.

## Cross-platform foundation

Do not duplicate prompts, model catalog entries, settings defaults, cleanup sanitizer behavior, language routing, or paste formatting in platform-specific code. Put product behavior in shared crates/resources and keep OS behavior in platform adapters.

## Pull request guidelines

Before opening a pull request:

1. Keep the change focused.
2. Add or update tests when practical.
3. Run the relevant checks.
4. Include screenshots or recordings for visible UI changes.
5. Explain the behavior change clearly.
6. Avoid including generated build artifacts, local model files, `.env` files, logs, or private data.

## Coding style

Use the style already present in the project:

- TypeScript: strict ES modules, 2-space indentation, camelCase.
- Rust: standard `rustfmt` style, snake_case modules/functions.
- Swift: focused types, descriptive XCTest names.
- UI copy: direct, friendly, and privacy-conscious.

## Local-first privacy expectations

Parrot should remain local-first. Avoid adding telemetry, analytics, cloud processing, or network requests unless the change is explicitly discussed and documented.

If a contribution changes data flow, storage, permissions, networking, clipboard behavior, model downloads, or update behavior, update [PRIVACY.md](PRIVACY.md).

## License

By contributing to Parrot, you agree that your contributions are licensed under the repository's MIT License.
