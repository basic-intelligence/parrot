# Contributing to Parrot

Thanks for wanting to help make Parrot better.

Parrot is a local-first, open-source dictation app. Contributions are welcome across code, design, docs, testing, packaging, accessibility, privacy review, language support, and platform support.

## Ways to contribute

Helpful contributions include:

- Bug reports with clear reproduction steps.
- Feature requests that explain the user problem.
- Pull requests that fix bugs or improve the app.
- Testing on different Macs, microphones, languages, and macOS versions.
- Documentation improvements.
- Privacy, security, and accessibility feedback.
- Windows and Linux sidecar work.

## Before opening an issue

Please check existing issues first to avoid duplicates.

When reporting a bug, include:

- Parrot version.
- macOS version.
- Apple Silicon or Intel.
- What you expected to happen.
- What actually happened.
- Steps to reproduce.
- Screenshots or logs, if helpful.

Please do not include private transcripts, recordings, names, API keys, secrets, or sensitive information in public issues.

For security or privacy vulnerabilities, do not open a public issue. See [SECURITY.md](SECURITY.md).

## Development setup

Parrot is a Tauri 2 desktop app with a TypeScript/Vite frontend, Rust host, and Swift macOS native sidecar.

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

Run Rust tests:

```sh
cargo test --manifest-path src-tauri/Cargo.toml
```

Run Swift tests:

```sh
swift test --package-path native-core/macos
```

Build a packaged app:

```sh
npm run build
```

## Project structure

- `src/` — TypeScript frontend.
- `src-tauri/src/` — Rust/Tauri host.
- `native-core/macos/` — Swift macOS sidecar.
- `native-core/shared/` — Shared prompts and language catalog.
- `public/` — Static frontend assets.
- `src-tauri/icons/` — App and tray icons.
- `src-tauri/capabilities/` — Tauri permissions.

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
