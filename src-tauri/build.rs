use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // The Swift ML sidecar is macOS-only. It must exist before
    // `tauri_build::build()` runs, because that step copies every
    // `bundle.externalBin` (declared in tauri.macos.conf.json) next to the
    // app binary and fails if one is missing.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_ml_sidecar();
        build_cli_sidecar();
    }
    tauri_build::build()
}

/// Builds the independent CLI package, not the Tauri application. A separate
/// target directory avoids a nested Cargo lock on the app build. The executable
/// also contains its headless service, so no GUI/Swift runtime is needed when
/// installing just this sidecar.
fn build_cli_sidecar() {
    println!("cargo:rerun-if-changed=cli/Cargo.toml");
    println!("cargo:rerun-if-changed=cli/src");
    println!("cargo:rerun-if-changed=core/Cargo.toml");
    println!("cargo:rerun-if-changed=core/src");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let target = env::var("TARGET").expect("TARGET");
    let output = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("cli-target");
    let status = Command::new(env::var_os("CARGO").expect("CARGO"))
        .args([
            "build",
            "--locked",
            "--release",
            "--package",
            "openrize-cli",
            "--target",
        ])
        .arg(&target)
        .arg("--manifest-path")
        .arg(manifest.join("cli/Cargo.toml"))
        .arg("--target-dir")
        .arg(&output)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .status()
        .expect("could not build the CLI sidecar");
    assert!(status.success(), "CLI sidecar build failed: {status}");
    let built = output.join(&target).join("release/rize");
    let staged = manifest.join("binaries").join(format!("rize-{target}"));
    if !matches!((fs::read(&built), fs::read(&staged)), (Ok(a), Ok(b)) if a == b) {
        fs::create_dir_all(staged.parent().expect("binaries dir")).expect("binaries dir");
        fs::copy(built, staged).expect("could not stage the CLI sidecar");
    }
}

/// Builds `swift/` (the `openrize-ml` executable) and stages it as
/// `binaries/openrize-ml-<target triple>`, the name Tauri's externalBin
/// convention expects.
fn build_ml_sidecar() {
    println!("cargo:rerun-if-changed=swift/Package.swift");
    println!("cargo:rerun-if-changed=swift/Sources");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let package = manifest_dir.join("swift");
    let target = env::var("TARGET").expect("TARGET");
    let arch = if target.starts_with("x86_64") {
        "x86_64"
    } else {
        "arm64"
    };
    let args = [
        "build",
        "-c",
        "release",
        "--arch",
        arch,
        "--product",
        "openrize-ml",
        "--package-path",
    ];

    let status = Command::new("swift")
        .args(args)
        .arg(&package)
        .status()
        .unwrap_or_else(|error| panic!("could not run `swift build` for the ML sidecar: {error}"));
    if !status.success() {
        panic!(
            "`swift build` for the ML sidecar failed ({status}). It needs Xcode 26+ \
             (the Foundation Models SDK); see src-tauri/swift/."
        );
    }

    let output = Command::new("swift")
        .args(args)
        .arg(&package)
        .arg("--show-bin-path")
        .output()
        .expect("could not ask swift for the sidecar bin path");
    let bin_dir = String::from_utf8(output.stdout).expect("utf-8 bin path");
    let built = Path::new(bin_dir.trim()).join("openrize-ml");

    let staged = manifest_dir
        .join("binaries")
        .join(format!("openrize-ml-{target}"));
    // Copy only on change: tauri-build watches the staged file, so rewriting
    // an identical binary would rerun this script on every build.
    let unchanged = matches!(
        (fs::read(&built), fs::read(&staged)),
        (Ok(fresh), Ok(current)) if fresh == current
    );
    if !unchanged {
        fs::create_dir_all(staged.parent().expect("binaries dir"))
            .expect("could not create src-tauri/binaries");
        fs::copy(&built, &staged).unwrap_or_else(|error| {
            panic!(
                "could not stage the ML sidecar at {}: {error}",
                staged.display()
            )
        });
    }
}
