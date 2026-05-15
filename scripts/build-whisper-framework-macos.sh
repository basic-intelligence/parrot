#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CORE_DIR="$ROOT_DIR/native-core/macos"
VENDOR_DIR="$CORE_DIR/.vendor"
WHISPER_VERSION="${WHISPER_VERSION:-v1.8.4}"
MACOS_MINIMUM_VERSION="${MACOS_MINIMUM_VERSION:-13.3}"

SRC_DIR="$VENDOR_DIR/whisper.cpp-$WHISPER_VERSION"
BUILD_DIR="$VENDOR_DIR/whisper-build-macos-cpu"
OUT_XCFRAMEWORK="$VENDOR_DIR/WhisperFramework.xcframework"

command -v git >/dev/null || { echo "git is required" >&2; exit 1; }
command -v cmake >/dev/null || { echo "cmake is required; install with: brew install cmake" >&2; exit 1; }
command -v xcodebuild >/dev/null || { echo "xcodebuild is required" >&2; exit 1; }
command -v libtool >/dev/null || { echo "libtool is required" >&2; exit 1; }

mkdir -p "$VENDOR_DIR"

if [[ ! -d "$SRC_DIR/.git" ]]; then
  git clone --depth 1 --branch "$WHISPER_VERSION" \
    https://github.com/ggml-org/whisper.cpp.git "$SRC_DIR"
fi

rm -rf "$BUILD_DIR" "$OUT_XCFRAMEWORK"

COMMON_C_FLAGS="-Wno-macro-redefined -Wno-shorten-64-to-32 -Wno-unused-command-line-argument"
COMMON_CXX_FLAGS="$COMMON_C_FLAGS"

cmake -B "$BUILD_DIR" -G Xcode \
  -S "$SRC_DIR" \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MINIMUM_VERSION" \
  -DCMAKE_OSX_ARCHITECTURES="arm64;x86_64" \
  -DCMAKE_C_FLAGS="$COMMON_C_FLAGS" \
  -DCMAKE_CXX_FLAGS="$COMMON_CXX_FLAGS" \
  -DBUILD_SHARED_LIBS=OFF \
  -DWHISPER_BUILD_EXAMPLES=OFF \
  -DWHISPER_BUILD_TESTS=OFF \
  -DWHISPER_BUILD_SERVER=OFF \
  -DWHISPER_COREML=OFF \
  -DWHISPER_COREML_ALLOW_FALLBACK=OFF \
  -DGGML_METAL=OFF \
  -DGGML_METAL_EMBED_LIBRARY=OFF \
  -DGGML_ACCELERATE=ON \
  -DGGML_BLAS=ON \
  -DGGML_BLAS_VENDOR=Apple \
  -DGGML_NATIVE=OFF \
  -DGGML_OPENMP=OFF

cmake --build "$BUILD_DIR" --config Release -- -quiet

FRAMEWORK_DIR="$BUILD_DIR/framework/whisper.framework"
FRAMEWORK_A="$FRAMEWORK_DIR/Versions/A"
HEADERS_DIR="$FRAMEWORK_A/Headers"
MODULES_DIR="$FRAMEWORK_A/Modules"
RESOURCES_DIR="$FRAMEWORK_A/Resources"

mkdir -p "$HEADERS_DIR" "$MODULES_DIR" "$RESOURCES_DIR"

ln -sf A "$FRAMEWORK_DIR/Versions/Current"
ln -sf Versions/Current/Headers "$FRAMEWORK_DIR/Headers"
ln -sf Versions/Current/Modules "$FRAMEWORK_DIR/Modules"
ln -sf Versions/Current/Resources "$FRAMEWORK_DIR/Resources"
ln -sf Versions/Current/whisper "$FRAMEWORK_DIR/whisper"

cp "$SRC_DIR/include/whisper.h" "$HEADERS_DIR/"
cp "$SRC_DIR/ggml/include/ggml.h" "$HEADERS_DIR/"
cp "$SRC_DIR/ggml/include/ggml-alloc.h" "$HEADERS_DIR/"
cp "$SRC_DIR/ggml/include/ggml-backend.h" "$HEADERS_DIR/"
cp "$SRC_DIR/ggml/include/ggml-cpu.h" "$HEADERS_DIR/"
cp "$SRC_DIR/ggml/include/ggml-blas.h" "$HEADERS_DIR/"
cp "$SRC_DIR/ggml/include/gguf.h" "$HEADERS_DIR/"

cat > "$MODULES_DIR/module.modulemap" <<'MODULEMAP'
framework module whisper {
  header "whisper.h"
  header "ggml.h"
  header "ggml-alloc.h"
  header "ggml-backend.h"
  header "ggml-cpu.h"
  header "ggml-blas.h"
  header "gguf.h"
  link "c++"
  link framework "Accelerate"
  export *
}
MODULEMAP

cat > "$RESOURCES_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>whisper</string>
    <key>CFBundleIdentifier</key>
    <string>org.ggml.whisper</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>whisper</string>
    <key>CFBundlePackageType</key>
    <string>FMWK</string>
    <key>CFBundleShortVersionString</key>
    <string>${WHISPER_VERSION#v}</string>
    <key>CFBundleVersion</key>
    <string>${WHISPER_VERSION#v}</string>
    <key>MinimumOSVersion</key>
    <string>${MACOS_MINIMUM_VERSION}</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
      <string>MacOSX</string>
    </array>
    <key>DTPlatformName</key>
    <string>macosx</string>
  </dict>
</plist>
PLIST

libs=(
  "$BUILD_DIR/src/Release/libwhisper.a"
  "$BUILD_DIR/ggml/src/Release/libggml.a"
  "$BUILD_DIR/ggml/src/Release/libggml-base.a"
  "$BUILD_DIR/ggml/src/Release/libggml-cpu.a"
  "$BUILD_DIR/ggml/src/ggml-blas/Release/libggml-blas.a"
)

for lib in "${libs[@]}"; do
  if [[ ! -f "$lib" ]]; then
    echo "Required whisper/ggml static library not found: $lib" >&2
    exit 1
  fi
done

mkdir -p "$BUILD_DIR/temp"
libtool -static -o "$BUILD_DIR/temp/combined.a" "${libs[@]}"

xcrun -sdk macosx clang++ -dynamiclib \
  -isysroot "$(xcrun --sdk macosx --show-sdk-path)" \
  -arch arm64 \
  -arch x86_64 \
  -mmacosx-version-min="$MACOS_MINIMUM_VERSION" \
  -Wl,-force_load,"$BUILD_DIR/temp/combined.a" \
  -framework Foundation \
  -framework Accelerate \
  -install_name "@rpath/whisper.framework/Versions/Current/whisper" \
  -o "$FRAMEWORK_A/whisper"

if otool -L "$FRAMEWORK_A/whisper" | grep -E "Metal.framework|CoreML.framework"; then
  echo "whisper.framework unexpectedly links Metal or CoreML" >&2
  exit 1
fi

xcodebuild -create-xcframework \
  -framework "$FRAMEWORK_DIR" \
  -output "$OUT_XCFRAMEWORK" >/dev/null

echo "Built CPU-only whisper XCFramework: $OUT_XCFRAMEWORK"
