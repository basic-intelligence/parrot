# Parrot Native Core for Linux

## Scope

Parrot's Linux build targets x86_64 Linux with CPU-only models, AppImage and
`.deb` packaging. CUDA, Flatpak, Snap, and AUR packaging are outside this
native-core package.

## Supported Distros

Use Ubuntu 22.04 or Debian 12 as the first packaging baseline. Newer distros may
work, but release artifacts should be built on the older baseline to avoid
raising the glibc requirement.

## Build

Install the Tauri Linux prerequisites plus X11 paste support libraries:

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libpulse-dev \
  libasound2-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  cmake \
  pkg-config
```

Build the Linux sidecar:

```bash
npm run build:core:linux
```

This creates:

```text
src-tauri/binaries/parrot-core-cpu-x86_64-unknown-linux-gnu
```

Build the packaged Linux app:

```bash
npm run build:linux
```

The Tauri Linux config builds AppImage and `.deb` artifacts and includes the
`parrot-core-cpu` sidecar.

For unsigned CI or local package validation on Linux, use:

```bash
npm run build:linux:qa
```

The QA config builds the same AppImage and `.deb` bundle targets but disables
updater artifact signing.

## Test

```bash
cargo test --manifest-path native-core/linux/Cargo.toml
cargo test --workspace
npm run build:ui
```

Normal CI tests do not require a microphone or desktop session. Real shortcut
runtime tests are ignored by default and should be run manually on X11 and
Wayland desktops.

## Linux shortcut backends

Parrot supports multiple Linux shortcut backends:

1. X11 global grabs.
2. Wayland compositor keybindings for Hyprland/Omarchy, Sway, River, and Niri.
3. XDG GlobalShortcuts portal runtime registration.
4. XDG GlobalShortcuts portal v2 shortcut configuration.
5. evdev kernel-level fallback.

On Hyprland/Omarchy, the recommended setup is compositor-managed shortcuts:
Parrot writes `~/.config/hypr/parrot.conf` and sources it from
`~/.config/hypr/hyprland.conf`.

Default Linux shortcuts:

- Hands-free: Ctrl + Space
- Push-to-talk: F9

For GNOME, KDE, or desktops without compositor release bindings, enable evdev:

```bash
sudo usermod -aG input $USER
```

Then log out and back in.

## Runtime Dependencies

The `.deb` declares `xdotool` and `libxdo3` for X11 paste injection, plus
`libpulse0`, `libasound2`, and `pipewire-pulse | pulseaudio` for desktop
microphone capture.
On Wayland, Parrot uses compositor shortcuts, the XDG Desktop Portal
GlobalShortcuts API, or evdev for shortcuts and looks for known paste helpers in
system locations.

Wayland paste helpers, in priority order:

- `hyprctl` for Hyprland
- `wtype` for wlroots-compatible compositors
- `dotool`
- `ydotool`
- clipboard-only fallback
- `wl-copy` and `wl-paste` for clipboard access

Parrot does not rely on shell startup files to populate `PATH`.

Sound playback is currently a no-op on Linux. Recording does not fail if sound
playback is unavailable.

## Model Cache

The Tauri host passes platform paths to the sidecar. Linux model files are stored
under the app data model cache:

```text
whisper-models/
cleanup-models/
```

Speech models use the existing shared Whisper.cpp GGML catalog entries. Cleanup
models use the existing shared llama.cpp GGUF catalog entries.

## Troubleshooting

- If no microphones appear, verify that PipeWire or PulseAudio can see the input
  device and that desktop privacy settings allow microphone access.
- If shortcuts do not start on Hyprland/Omarchy, click "Install/update Linux
  shortcuts" in Parrot and confirm `~/.config/hypr/hyprland.conf` sources
  `~/.config/hypr/parrot.conf`.
- If shortcuts do not start on other Wayland desktops, use the portal backend
  when available or enable evdev with `sudo usermod -aG input $USER`, then log
  out and back in.
- If paste fails on X11, confirm `xdotool` is available.
- If paste fails on Wayland, confirm `wl-copy` is available and install a helper
  supported by your compositor: `hyprctl`, `wtype`, `dotool`, or `ydotool`.
- If models stay missing, delete partial `.download` files from the speech or
  cleanup model cache and download again.
