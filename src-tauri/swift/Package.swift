// swift-tools-version: 6.0
// The on-device ML sidecar (`openrize-ml`). `src-tauri/build.rs` builds it and
// stages it as the Tauri `externalBin` declared in `tauri.macos.conf.json`.
import PackageDescription

let package = Package(
    name: "openrize-ml",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "openrize-ml",
            path: "Sources/openrize-ml",
            linkerSettings: [
                // Foundation Models only exists on macOS 26+. Weak-linking it
                // lets the same binary start on macOS 14/15, where the sidecar
                // reports the LLM as unsupported and serves T1 only.
                .unsafeFlags(["-Xlinker", "-weak_framework", "-Xlinker", "FoundationModels"]),
            ]
        ),
    ]
)
