// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Dictator",
    platforms: [.macOS(.v15)],
    dependencies: [
        // Third-party dependencies, pinned exactly; bump deliberately after reviewing the diff.
        .package(url: "https://github.com/argmaxinc/WhisperKit.git", exact: "1.1.1"),
        // Parakeet TDT (Core ML). The NemoTextProcessing trait (a prebuilt binary for text
        // normalisation we don't use) is turned off, so only FluidAudio's own source is linked.
        .package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.17.7", traits: []),
        // Our own shared core (state machine, shortcut model, transcript cleanup), also used by the
        // iOS app. Pinned exactly like everything else.
        .package(url: "https://github.com/captains-chest/DictationCore.git", exact: "0.3.0"),
    ],
    targets: [
        // Loads and runs one speech model; adapters supply only the engine.
        .target(
            name: "ModelSession",
            dependencies: [.product(name: "DictationCore", package: "DictationCore")]
        ),

        // The single seam where WhisperKit is used.
        .target(
            name: "WhisperTranscription",
            dependencies: [
                .product(name: "DictationCore", package: "DictationCore"),
                .product(name: "WhisperKit", package: "WhisperKit"),
                "ModelSession",
            ]
        ),

        // The single seam where FluidAudio is used (Parakeet TDT 0.6B v2, English).
        .target(
            name: "ParakeetTranscription",
            dependencies: [
                .product(name: "DictationCore", package: "DictationCore"),
                .product(name: "FluidAudio", package: "FluidAudio"),
                "ModelSession",
            ]
        ),

        // Optional transcript cleanup (ADR 0011): the port, the S1-mini prompt format, chunking and
        // the decorator that falls back to the raw transcript. Pure logic, no model runtime.
        .target(
            name: "TranscriptCleanup",
            dependencies: [.product(name: "DictationCore", package: "DictationCore")]
        ),

        // llama.cpp, the official prebuilt xcframework (MIT, Metal on Apple Silicon). Pinned to one
        // release by URL and checksum; bump deliberately after reviewing the release.
        .binaryTarget(
            name: "llama",
            url: "https://github.com/ggml-org/llama.cpp/releases/download/b11838/llama-b11838-xcframework.zip",
            checksum: "37cffab2499d4464defe050486f040c116156df6f840b63c9c44fd004f42807e"
        ),

        // The single seam where llama.cpp is used (S1-mini by Superwhisper, transcript cleanup).
        .target(
            name: "S1MiniCleanup",
            dependencies: ["TranscriptCleanup", "llama"]
        ),

        // Release lookup, signature policy and the replace step. Shared with the updater so the
        // unsandboxed helper checks the same facts the app does.
        .target(
            name: "AppUpdate",
            dependencies: [.product(name: "DictationCore", package: "DictationCore")]
        ),

        // macOS app: hotkey, microphone, text insertion, menu bar, overlay, settings.
        .executableTarget(
            name: "Dictator",
            dependencies: [
                .product(name: "DictationCore", package: "DictationCore"),
                "WhisperTranscription",
                "ParakeetTranscription",
                "ModelSession",
                "AppUpdate",
                "TranscriptCleanup",
                "S1MiniCleanup",
            ]
        ),

        // Unsandboxed XPC service embedded in the app. Replaces Dictator.app and nothing else.
        .executableTarget(
            name: "DictatorUpdater",
            dependencies: ["AppUpdate"]
        ),

        // App-level logic with injected fakes (e.g. keystroke pacing, issue #43).
        .testTarget(
            name: "DictatorTests",
            dependencies: [
                .product(name: "DictationCore", package: "DictationCore"),
                "Dictator",
                "AppUpdate",
                "WhisperTranscription",
                "TranscriptCleanup",
            ]
        ),
        .testTarget(name: "ModelSessionTests", dependencies: ["ModelSession", .product(name: "DictationCore", package: "DictationCore")]),
        .testTarget(name: "ParakeetTranscriptionTests", dependencies: ["ParakeetTranscription", "ModelSession"]),
        .testTarget(name: "TranscriptCleanupTests", dependencies: ["TranscriptCleanup", .product(name: "DictationCore", package: "DictationCore")]),
        .testTarget(name: "S1MiniCleanupTests", dependencies: ["S1MiniCleanup", "TranscriptCleanup"]),
        .testTarget(name: "WhisperTranscriptionTests", dependencies: [.product(name: "DictationCore", package: "DictationCore"), "WhisperTranscription", "ModelSession"]),
    ]
)
