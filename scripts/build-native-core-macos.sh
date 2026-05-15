#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CORE_DIR="$ROOT_DIR/native-core/macos"
BIN_DIR="$ROOT_DIR/src-tauri/binaries"
APP_VERSION="$(node -e 'const fs = require("fs"); const config = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); process.stdout.write(config.version);' "$ROOT_DIR/src-tauri/tauri.conf.json")"
MACOS_MINIMUM_VERSION="${MACOS_MINIMUM_VERSION:-13.3}"
export MACOSX_DEPLOYMENT_TARGET="$MACOS_MINIMUM_VERSION"

build_core_arch() {
  local arch="$1"

  swift build \
    --package-path "$CORE_DIR" \
    -c release \
    --arch "$arch" >&2
}

find_release_binary() {
  local arch="$1"
  local name="$2"
  local binary

  binary="$CORE_DIR/.build/$arch-apple-macosx/release/$name"
  if [[ ! -f "$binary" ]]; then
    binary="$(
      find "$CORE_DIR/.build" \
        -type f \
        -name "$name" \
        -path "*/$arch-apple-macosx/release/$name" | head -n 1
    )"
  fi

  if [[ -z "$binary" ]]; then
    echo "$name binary not found for $arch" >&2
    exit 1
  fi

  printf '%s\n' "$binary"
}

"$ROOT_DIR/scripts/build-whisper-framework-macos.sh"

build_core_arch arm64
build_core_arch x86_64
ARM_BIN="$(find_release_binary arm64 parrot-core)"
X86_BIN="$(find_release_binary x86_64 parrot-core)"
ARM_WHISPER_BIN="$(find_release_binary arm64 parrot-whisper)"
X86_WHISPER_BIN="$(find_release_binary x86_64 parrot-whisper)"

UNIVERSAL_DIR="$CORE_DIR/.build-universal/release"
mkdir -p "$UNIVERSAL_DIR"
CORE_BIN="$UNIVERSAL_DIR/parrot-core"
WHISPER_HELPER_BIN="$UNIVERSAL_DIR/parrot-whisper"
lipo -create "$ARM_BIN" "$X86_BIN" -output "$CORE_BIN"
lipo -create "$ARM_WHISPER_BIN" "$X86_WHISPER_BIN" -output "$WHISPER_HELPER_BIN"
lipo -info "$CORE_BIN"
lipo -info "$WHISPER_HELPER_BIN"

mkdir -p "$BIN_DIR"
FRAMEWORK_SEARCH_PATHS=()
for build_dir in "$CORE_DIR/.build" "$CORE_DIR/.build-arm64" "$CORE_DIR/.build-x86_64"; do
  if [[ -d "$build_dir" ]]; then
    FRAMEWORK_SEARCH_PATHS+=("$build_dir")
  fi
done

LLAMA_FRAMEWORK=""
WHISPER_FRAMEWORK=""
if (( ${#FRAMEWORK_SEARCH_PATHS[@]} > 0 )); then
  LLAMA_FRAMEWORK="$(
    find "${FRAMEWORK_SEARCH_PATHS[@]}" \
      -path '*/release/llama.framework' \
      -type d 2>/dev/null | head -n 1
  )"
  WHISPER_FRAMEWORK="$(
    find "${FRAMEWORK_SEARCH_PATHS[@]}" \
      -path '*/release/whisper.framework' \
      -type d 2>/dev/null | head -n 1
  )"
fi

if [[ -z "$WHISPER_FRAMEWORK" ]]; then
  WHISPER_FRAMEWORK="$(
    find "$CORE_DIR/.vendor/WhisperFramework.xcframework" \
      -path '*/whisper.framework' \
      -type d 2>/dev/null | head -n 1
  )"
fi

if [[ -d "$LLAMA_FRAMEWORK" || -d "$WHISPER_FRAMEWORK" ]]; then
  if ! otool -l "$CORE_BIN" | grep -q '@loader_path/../Frameworks'; then
    install_name_tool -add_rpath '@loader_path/../Frameworks' "$CORE_BIN"
  fi
  if ! otool -l "$WHISPER_HELPER_BIN" | grep -q '@loader_path/../Frameworks'; then
    install_name_tool -add_rpath '@loader_path/../Frameworks' "$WHISPER_HELPER_BIN"
  fi
fi
if [[ ! -d "$LLAMA_FRAMEWORK" ]]; then
  echo "warning: llama.framework not found; native cleanup model may fail to load at runtime" >&2
fi
if [[ ! -d "$WHISPER_FRAMEWORK" ]]; then
  echo "warning: whisper.framework not found; Intel speech model may fail to load at runtime" >&2
fi

rm -rf \
  "$BIN_DIR/llama.framework" \
  "$BIN_DIR/whisper.framework" \
  "$ROOT_DIR/src-tauri/target/Frameworks/llama.framework" \
  "$ROOT_DIR/src-tauri/target/Frameworks/whisper.framework" \
  "$ROOT_DIR/src-tauri/target/debug/llama.framework" \
  "$ROOT_DIR/src-tauri/target/debug/whisper.framework" \
  "$ROOT_DIR/src-tauri/target/release/llama.framework" \
  "$ROOT_DIR/src-tauri/target/release/whisper.framework"

rm -f "$BIN_DIR"/parrot-core-* "$BIN_DIR/parrot-core"

HELPER_APP="$BIN_DIR/Parrot.app"
HELPER_CONTENTS="$HELPER_APP/Contents"
HELPER_MACOS="$HELPER_CONTENTS/MacOS"
HELPER_RESOURCES="$HELPER_CONTENTS/Resources"
HELPER_FRAMEWORKS="$HELPER_CONTENTS/Frameworks"
HELPER_ENTITLEMENTS="$ROOT_DIR/src-tauri/ParrotCore.entitlements"

rm -rf "$HELPER_APP"
mkdir -p "$HELPER_MACOS" "$HELPER_RESOURCES" "$HELPER_FRAMEWORKS"

cp "$CORE_BIN" "$HELPER_MACOS/parrot-core"
chmod +x "$HELPER_MACOS/parrot-core"
cp "$WHISPER_HELPER_BIN" "$HELPER_MACOS/parrot-whisper"
chmod +x "$HELPER_MACOS/parrot-whisper"

if otool -L "$HELPER_MACOS/parrot-core" | grep -q '@rpath/llama.framework/Versions/Current/llama'; then
  install_name_tool \
    -change '@rpath/llama.framework/Versions/Current/llama' \
    '@rpath/llama.framework/llama' \
    "$HELPER_MACOS/parrot-core"
fi
if otool -L "$HELPER_MACOS/parrot-core" | grep -q '@rpath/whisper.framework/Versions/Current/whisper'; then
  install_name_tool \
    -change '@rpath/whisper.framework/Versions/Current/whisper' \
    '@rpath/whisper.framework/whisper' \
    "$HELPER_MACOS/parrot-core"
fi
if otool -L "$HELPER_MACOS/parrot-whisper" | grep -q '@rpath/whisper.framework/Versions/Current/whisper'; then
  install_name_tool \
    -change '@rpath/whisper.framework/Versions/Current/whisper' \
    '@rpath/whisper.framework/whisper' \
    "$HELPER_MACOS/parrot-whisper"
fi

cp "$ROOT_DIR/src-tauri/icons/icon.icns" "$HELPER_RESOURCES/icon.icns"

cat > "$HELPER_CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>CFBundleName</key>
    <string>Parrot Core</string>
    <key>CFBundleDisplayName</key>
    <string>Parrot Core</string>
    <key>CFBundleIdentifier</key>
    <string>in.basic.parrot.core</string>
    <key>CFBundleExecutable</key>
    <string>parrot-core</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${APP_VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${APP_VERSION}</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>CFBundleIconName</key>
    <string>icon</string>
    <key>LSMinimumSystemVersion</key>
    <string>${MACOS_MINIMUM_VERSION}</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>Parrot records your voice to transcribe dictation locally.</string>
    <key>NSInputMonitoringUsageDescription</key>
    <string>Parrot Core listens for your configured shortcut while you use other apps.</string>
  </dict>
</plist>
PLIST

if [[ -d "$LLAMA_FRAMEWORK" ]]; then
  rm -rf "$HELPER_FRAMEWORKS/llama.framework"
  cp -R "$LLAMA_FRAMEWORK" "$HELPER_FRAMEWORKS/llama.framework"
fi
if [[ -d "$WHISPER_FRAMEWORK" ]]; then
  rm -rf "$HELPER_FRAMEWORKS/whisper.framework"
  cp -R "$WHISPER_FRAMEWORK" "$HELPER_FRAMEWORKS/whisper.framework"
fi

HELPER_ARCHS="$(lipo -archs "$HELPER_MACOS/parrot-core")"
echo "Parrot Core architectures: $HELPER_ARCHS"
if [[ " $HELPER_ARCHS " != *" arm64 "* || " $HELPER_ARCHS " != *" x86_64 "* ]]; then
  echo "Parrot Core must be universal arm64 and x86_64" >&2
  exit 1
fi
WHISPER_HELPER_ARCHS="$(lipo -archs "$HELPER_MACOS/parrot-whisper")"
echo "Parrot Whisper architectures: $WHISPER_HELPER_ARCHS"
if [[ " $WHISPER_HELPER_ARCHS " != *" arm64 "* || " $WHISPER_HELPER_ARCHS " != *" x86_64 "* ]]; then
  echo "Parrot Whisper must be universal arm64 and x86_64" >&2
  exit 1
fi

LLAMA_BINARY="$HELPER_FRAMEWORKS/llama.framework/Versions/Current/llama"
if [[ -f "$LLAMA_BINARY" ]]; then
  LLAMA_ARCHS="$(lipo -archs "$LLAMA_BINARY")"
  echo "llama.framework architectures: $LLAMA_ARCHS"
  if [[ " $LLAMA_ARCHS " != *" arm64 "* || " $LLAMA_ARCHS " != *" x86_64 "* ]]; then
    echo "llama.framework must be universal arm64 and x86_64" >&2
    exit 1
  fi
fi
WHISPER_BINARY="$HELPER_FRAMEWORKS/whisper.framework/Versions/Current/whisper"
if [[ -f "$WHISPER_BINARY" ]]; then
  WHISPER_ARCHS="$(lipo -archs "$WHISPER_BINARY")"
  echo "whisper.framework architectures: $WHISPER_ARCHS"
  if [[ " $WHISPER_ARCHS " != *" arm64 "* || " $WHISPER_ARCHS " != *" x86_64 "* ]]; then
    echo "whisper.framework must be universal arm64 and x86_64" >&2
    exit 1
  fi
  if otool -L "$WHISPER_BINARY" | grep -E "Metal.framework|CoreML.framework"; then
    echo "whisper.framework must be CPU-only for Intel compatibility" >&2
    exit 1
  fi
fi

if command -v codesign >/dev/null 2>&1; then
  if [[ "${CI:-}" == "true" && -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    echo "APPLE_SIGNING_IDENTITY is required in CI" >&2
    exit 1
  fi

  if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    if [[ -d "$HELPER_FRAMEWORKS/llama.framework" ]]; then
      codesign \
        --force \
        --timestamp \
        --options runtime \
        --sign "$APPLE_SIGNING_IDENTITY" \
        "$HELPER_FRAMEWORKS/llama.framework"
    fi
    if [[ -d "$HELPER_FRAMEWORKS/whisper.framework" ]]; then
      codesign \
        --force \
        --timestamp \
        --options runtime \
        --sign "$APPLE_SIGNING_IDENTITY" \
        "$HELPER_FRAMEWORKS/whisper.framework"
    fi

    codesign \
      --force \
      --timestamp \
      --options runtime \
      --sign "$APPLE_SIGNING_IDENTITY" \
      "$HELPER_MACOS/parrot-whisper"

    codesign \
      --force \
      --timestamp \
      --options runtime \
      --entitlements "$HELPER_ENTITLEMENTS" \
      --sign "$APPLE_SIGNING_IDENTITY" \
      "$HELPER_MACOS/parrot-core"

    codesign \
      --force \
      --timestamp \
      --options runtime \
      --entitlements "$HELPER_ENTITLEMENTS" \
      --sign "$APPLE_SIGNING_IDENTITY" \
      "$HELPER_APP"

    codesign --verify --deep --strict --verbose=2 "$HELPER_APP"
  else
    if [[ -d "$HELPER_FRAMEWORKS/llama.framework" ]]; then
      codesign --force --sign - "$HELPER_FRAMEWORKS/llama.framework"
    fi

    if [[ -d "$HELPER_FRAMEWORKS/whisper.framework" ]]; then
      codesign --force --sign - "$HELPER_FRAMEWORKS/whisper.framework"
    fi

    codesign --force --sign - "$HELPER_MACOS/parrot-whisper"
    codesign --force --entitlements "$HELPER_ENTITLEMENTS" --sign - "$HELPER_MACOS/parrot-core"
    codesign --force --entitlements "$HELPER_ENTITLEMENTS" --sign - "$HELPER_APP"
    codesign --verify --deep --strict --verbose=2 "$HELPER_APP" || true
  fi
fi

ENTITLEMENTS_DUMP="$(mktemp)"
codesign -d --entitlements :- "$HELPER_MACOS/parrot-core" > "$ENTITLEMENTS_DUMP" 2>/dev/null || true

if ! grep -q "com.apple.security.device.audio-input" "$ENTITLEMENTS_DUMP"; then
  echo "Parrot Core is missing com.apple.security.device.audio-input" >&2
  cat "$ENTITLEMENTS_DUMP" >&2
  rm -f "$ENTITLEMENTS_DUMP"
  exit 1
fi

rm -f "$ENTITLEMENTS_DUMP"

echo "Installed helper app: $HELPER_APP"
