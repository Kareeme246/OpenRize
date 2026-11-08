use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

fn main() {
    // The Swift ML sidecar is macOS-only. Each externalBin must exist before
    // `tauri_build::build()`, which copies it next to the app binary and fails if one is missing.
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "macos" {
        build_ml_sidecar();
    }
    if os == "macos" || os == "windows" {
        build_cli_sidecar();
        stage_llama_server();
    }
    tauri_build::build()
}

/// Builds the CLI package, not the Tauri application. A separate target
/// directory avoids a nested Cargo lock on the app build.
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
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let built = output.join(&target).join(format!("release/rize{suffix}"));
    let staged = manifest
        .join("binaries")
        .join(format!("rize-{target}{suffix}"));
    if !matches!((fs::read(&built), fs::read(&staged)), (Ok(a), Ok(b)) if a == b) {
        fs::create_dir_all(staged.parent().expect("binaries dir")).expect("binaries dir");
        fs::copy(built, &staged).expect("could not stage the CLI sidecar");
    }
}

/// Builds `swift/` (the `openrize-ml` executable) and stages it as `binaries/openrize-ml-<target
/// triple>`.
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

/// The one llama.cpp build the golden-set benchmark ran on; bump it only together with a re-run.
const LLAMA_BUILD: &str = "b11146";
/// Bump when the staging steps below change, so existing checkouts restage.
const STAGE_REVISION: &str = "2";

struct LlamaAsset {
    file: &'static str,
    sha256: &'static str,
}

const LLAMA_MACOS_ARM64: LlamaAsset = LlamaAsset {
    file: "llama-b11146-bin-macos-arm64.tar.gz",
    sha256: "1ad3f9eff80edb9dbef4259ad564d1720612ef7eea48fa4afed0e54f5f3d5711",
};
const LLAMA_WINDOWS_VULKAN_X64: LlamaAsset = LlamaAsset {
    file: "llama-b11146-bin-win-vulkan-x64.zip",
    sha256: "55a378aa095b466979d85075234f66d7655c7a7483222af0c006c0e55b4d7bd6",
};

/// Dylibs `llama-server` needs (`otool -L`, transitively), by install name.
const LLAMA_MACOS_DYLIBS: [&str; 10] = [
    "libllama-server-impl.dylib",
    "libllama-common.0.dylib",
    "libmtmd.0.dylib",
    "libllama.0.dylib",
    "libggml.0.dylib",
    "libggml-cpu.0.dylib",
    "libggml-blas.0.dylib",
    "libggml-metal.0.dylib",
    "libggml-rpc.0.dylib",
    "libggml-base.0.dylib",
];
/// DLLs `llama-server.exe` needs besides the `ggml-*` backends ggml loads from its own directory.
const LLAMA_WINDOWS_DLLS: [&str; 6] = [
    "llama-server-impl.dll",
    "llama-common.dll",
    "llama.dll",
    "mtmd.dll",
    "libomp.dll",
    "ggml.dll",
];

/// Stages the pinned `llama-server` runtime, bundled so it is never fetched at run time:
/// `binaries/llama-server-<triple>` (externalBin) and `binaries/llama-runtime-<os>/` (dylibs or DLLs
/// plus the license), which the platform configs ship beside it.
fn stage_llama_server() {
    println!("cargo:rerun-if-changed=llama.cpp-LICENSE");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let target = env::var("TARGET").expect("TARGET");
    let (asset, windows) = if target == "aarch64-apple-darwin" {
        (&LLAMA_MACOS_ARM64, false)
    } else if target.starts_with("x86_64-pc-windows") {
        (&LLAMA_WINDOWS_VULKAN_X64, true)
    } else {
        panic!("no bundled llama.cpp {LLAMA_BUILD} runtime for target {target}");
    };
    let binaries = manifest.join("binaries");
    let runtime = binaries.join(if windows {
        "llama-runtime-windows"
    } else {
        "llama-runtime-macos"
    });
    let server = binaries.join(format!(
        "llama-server-{target}{}",
        if windows { ".exe" } else { "" }
    ));
    let stamp_path = runtime.with_extension("stamp");
    println!("cargo:rerun-if-changed={}", stamp_path.display());
    let stamp = format!("{LLAMA_BUILD} {} {STAGE_REVISION}", asset.sha256);
    let current = fs::read_to_string(&stamp_path).is_ok_and(|s| s == stamp)
        && server.exists()
        && runtime.is_dir();
    if !current {
        let archive = fetch_llama_archive(&binaries.join("llama-cache"), asset);
        let _ = fs::remove_file(&stamp_path);
        let _ = fs::remove_dir_all(&runtime);
        fs::create_dir_all(&runtime).expect("could not create the llama runtime dir");
        // The Windows archive ships no MIT license text, so both platforms use the committed copy.
        fs::copy(
            manifest.join("llama.cpp-LICENSE"),
            runtime.join("llama.cpp-LICENSE"),
        )
        .expect("could not stage the llama.cpp license");
        if windows {
            stage_llama_windows(&archive, &runtime, &server);
        } else {
            stage_llama_macos(&archive, &runtime, &server);
        }
        fs::write(&stamp_path, stamp).expect("could not write the llama runtime stamp");
    }
    if !windows {
        // tauri-build copies only the externalBin next to the dev binary; the staged server finds
        // its dylibs there through its `@loader_path` rpath.
        let profile_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"))
            .ancestors()
            .nth(3)
            .expect("target profile dir")
            .to_path_buf();
        for entry in fs::read_dir(&runtime).expect("llama runtime dir") {
            let entry = entry.expect("llama runtime entry");
            if entry.path().extension().is_some_and(|e| e == "dylib") {
                copy_if_changed(&entry.path(), &profile_dir.join(entry.file_name()));
            }
        }
    }
}

fn copy_if_changed(from: &Path, to: &Path) {
    if !matches!((fs::read(from), fs::read(to)), (Ok(a), Ok(b)) if a == b) {
        fs::copy(from, to)
            .unwrap_or_else(|error| panic!("could not copy {}: {error}", from.display()));
    }
}

/// Returns the verified archive, downloading it into `cache` when missing or corrupt.
fn fetch_llama_archive(cache: &Path, asset: &LlamaAsset) -> PathBuf {
    let path = cache.join(asset.file);
    if sha256_hex(&path).as_deref() == Some(asset.sha256) {
        return path;
    }
    fs::create_dir_all(cache).expect("could not create binaries/llama-cache");
    let part = cache.join(format!("{}.part", asset.file));
    let url = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_BUILD}/{}",
        asset.file
    );
    let status = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--retry", "3", "--output"])
        .arg(&part)
        .arg(&url)
        .status()
        .unwrap_or_else(|error| panic!("could not run curl to fetch {url}: {error}"));
    assert!(status.success(), "downloading {url} failed: {status}");
    let actual = sha256_hex(&part);
    if actual.as_deref() != Some(asset.sha256) {
        let _ = fs::remove_file(&part);
        panic!(
            "{url} has SHA-256 {actual:?}, expected {}; refusing to bundle it",
            asset.sha256
        );
    }
    fs::rename(&part, &path).expect("could not cache the llama.cpp archive");
    path
}

fn sha256_hex(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

#[cfg(not(unix))]
fn make_executable(_: &Path) {}

fn stage_llama_macos(archive: &Path, runtime: &Path, server: &Path) {
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let mut links: HashMap<String, String> = HashMap::new();
    let decoder = flate2::read::GzDecoder::new(File::open(archive).expect("llama archive"));
    for entry in tar::Archive::new(decoder)
        .entries()
        .expect("llama archive entries")
    {
        let mut entry = entry.expect("llama archive entry");
        let Some(name) = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        if !(name.ends_with(".dylib") || name == "llama-server") {
            continue;
        }
        if entry.header().entry_type().is_symlink() {
            if let Some(link) = entry.link_name().ok().flatten() {
                let link = link.file_name().map(|n| n.to_string_lossy().into_owned());
                links.extend(link.map(|l| (name, l)));
            }
        } else if entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("llama archive file");
            files.insert(name, bytes);
        }
    }
    let resolve = |name: &str| -> Vec<u8> {
        let mut current = name.to_string();
        for _ in 0..4 {
            if let Some(bytes) = files.get(&current) {
                return bytes.clone();
            }
            match links.get(&current) {
                Some(next) => current = next.clone(),
                None => break,
            }
        }
        panic!("llama.cpp {LLAMA_BUILD} archive has no {name}");
    };

    for name in LLAMA_MACOS_DYLIBS {
        let path = runtime.join(name);
        fs::write(&path, resolve(name)).expect("could not stage a llama dylib");
        make_executable(&path);
    }

    fs::write(server, resolve("llama-server")).expect("could not stage llama-server");
    make_executable(server);
    // The bundle keeps the dylibs in Contents/Frameworks; `@loader_path` stays for the dev copy.
    // Editing load commands voids the signature, and arm64 refuses to run an unsigned binary.
    run_tool(
        "install_name_tool",
        &[
            "-add_rpath".as_ref(),
            "@executable_path/../Frameworks".as_ref(),
            server.as_os_str(),
        ],
    );
    run_tool(
        "codesign",
        &[
            "--force".as_ref(),
            "--sign".as_ref(),
            "-".as_ref(),
            server.as_os_str(),
        ],
    );
}

fn run_tool(tool: &str, args: &[&std::ffi::OsStr]) {
    let status = Command::new(tool)
        .args(args)
        .status()
        .unwrap_or_else(|error| panic!("could not run {tool}: {error}"));
    assert!(status.success(), "{tool} failed: {status}");
}

fn stage_llama_windows(archive: &Path, runtime: &Path, server: &Path) {
    let mut zip = zip::ZipArchive::new(File::open(archive).expect("llama archive"))
        .expect("llama archive is not a zip");
    let mut found = Vec::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("llama archive entry");
        let name = file.name().to_string();
        let is_backend = name.starts_with("ggml-")
            && name.ends_with(".dll")
            && (name == "ggml-base.dll"
                || name == "ggml-rpc.dll"
                || name == "ggml-vulkan.dll"
                || name.starts_with("ggml-cpu-"));
        let destination = if name == "llama-server.exe" {
            server.to_path_buf()
        } else if name == "LICENSE-LLVM-OpenMP"
            || is_backend
            || LLAMA_WINDOWS_DLLS.contains(&name.as_str())
        {
            runtime.join(&name)
        } else {
            continue;
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("llama archive file");
        fs::write(&destination, bytes).expect("could not stage a llama file");
        found.push(name);
    }
    let required = LLAMA_WINDOWS_DLLS.iter().copied().chain([
        "llama-server.exe",
        "ggml-base.dll",
        "ggml-vulkan.dll",
    ]);
    for name in required {
        assert!(
            found.iter().any(|f| f == name),
            "llama.cpp {LLAMA_BUILD} archive has no {name}"
        );
    }
    assert!(
        found.iter().any(|f| f.starts_with("ggml-cpu-")),
        "llama.cpp {LLAMA_BUILD} archive has no CPU backend"
    );
}
