// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "ParrotCore",
    platforms: [.macOS("13.3")],
    products: [
        .executable(name: "parrot-core", targets: ["ParrotCore"]),
        .executable(name: "parrot-whisper", targets: ["ParrotWhisper"])
    ],
    dependencies: [
        .package(url: "https://github.com/argmaxinc/WhisperKit.git", from: "0.18.0")
    ],
    targets: [
        .binaryTarget(
            name: "llama",
            url: "https://github.com/ggml-org/llama.cpp/releases/download/b8933/llama-b8933-xcframework.zip",
            checksum: "9ae0aa407accf0bc48466636b08e4ffa89d0a9b4dc5dbde9fe265f43c29d08ec"
        ),
        .binaryTarget(
            name: "WhisperFramework",
            url: "https://github.com/ggml-org/whisper.cpp/releases/download/v1.8.4/whisper-v1.8.4-xcframework.zip",
            checksum: "1c7a93bd20fe4e57e0af12051ddb34b7a434dfc9acc02c8313393150b6d1821f"
        ),
        .executableTarget(
            name: "ParrotCore",
            dependencies: [
                .product(name: "WhisperKit", package: "WhisperKit"),
                "llama"
            ]
        ),
        .executableTarget(
            name: "ParrotWhisper",
            dependencies: [
                "WhisperCppBridge"
            ]
        ),
        .target(
            name: "WhisperCppBridge",
            dependencies: [
                "WhisperFramework"
            ]
        ),
        .testTarget(
            name: "ParrotCoreTests",
            dependencies: ["ParrotCore"]
        )
    ]
)
