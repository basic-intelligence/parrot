# Repository Guidelines

## Project Structure & Module Organization

Parrot is a Tauri 2 desktop app with a TypeScript/Vite frontend, Rust host, and Swift macOS native core. Frontend code lives in `src/`, with shared styling in `src/style.css` and recording UI assets in `src/recording.*`. Rust/Tauri code is in `src-tauri/src/`, with capabilities, permissions, icons, and bundle assets under `src-tauri/`. The macOS sidecar is in `native-core/macos/`; shared prompts and language data are in `native-core/shared/`. Static assets belong in `public/`, and helper scripts belong in `scripts/`.

## Build, Test, and Development Commands

- `npm ci` installs Node dependencies from `package-lock.json`.
- `npm run dev` starts the full Tauri development app.
- `npm run dev:ui` starts the Vite-only frontend on port `1420`.
- `npx serve . -l 3000` serves static files locally when a simple file server is enough.
- `npm run build:ui` builds the frontend.
- `npm run build:core:mac` builds the macOS native core.
- `npm run build` builds the packaged Tauri app.
- `cargo test --manifest-path src-tauri/Cargo.toml` runs Rust tests.
- `swift test --package-path native-core/macos` runs Swift tests.

## Coding Style & Naming Conventions

Follow the style already present. TypeScript uses strict ES modules, 2-space indentation, double quotes, camelCase variables, and PascalCase types. Rust should remain `rustfmt`-compatible, with snake_case modules and functions. Swift uses 4-space indentation, PascalCase types, and descriptive XCTest method names such as `testRemovesFullThinkBlock`. Keep UI copy short, direct, and privacy-conscious.

## Testing Guidelines

Add or update focused tests when changing behavior. Place Swift tests in `native-core/macos/Tests/ParrotCoreTests/`; keep names behavior-oriented. Run the relevant Rust, Swift, and frontend build checks before submitting. For UI changes, include screenshots or recordings in the pull request.

## Commit & Pull Request Guidelines

Use concise Conventional Commit-style messages seen in history, such as `docs: update license documentation`, `fix(onboarding): persist input monitoring state`, or `perf(macos): keep whisper helper warm`. Pull requests should include a summary, motivation, changes, testing commands, and privacy/security impact. Link related issues when applicable and keep generated artifacts, logs, recordings, model files, and secrets out of commits.

## Security, Configuration & Agent Notes

Parrot is local-first. Do not add telemetry, analytics, cloud processing, or network requests without explicit documentation and privacy review. Never read `.env` or `.env.local`; ask for specific values if needed. Use the `code` command for opening files, not `open`. On macOS, assume Apple Silicon unless stated otherwise.
