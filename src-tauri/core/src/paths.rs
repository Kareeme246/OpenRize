//! Where the installed app keeps its files, for clients that run without Tauri; mirrors Tauri 2's
//! `PathResolver` (`dirs::data_local_dir()` / `cache_dir()` plus the identifier). Windows data is
//! in Local AppData, not Roaming; see `data_location` for how older data moves.
use std::path::{Path, PathBuf};

use crate::APP_IDENTIFIER;

pub const DATABASE_FILE: &str = "activity.db";

/// Held exclusively by whichever process owns the stores: the running app for
/// its whole life, or `rize` while it edits them with the app closed.
pub const LOCK_FILE: &str = "app.lock";

pub const ACCOUNTS_FILE: &str = "accounts.json";

pub const SWITCH_FILE: &str = "switch.json";

/// Where Windows keeps settings (`settings.rs`), inside the data directory. It
/// is not data: `data_location` never counts, moves or removes it.
pub const CONFIG_DIR: &str = "config";

pub const ACCOUNTS_DIR: &str = "accounts";

/// Local AI model files (GGUF), in Local AppData and shared by every account. Like `config`, it
/// is not data: `data_location` never counts, moves or removes it, and no export includes it.
pub const MODELS_DIR: &str = "models";

/// An account's trained personal models, inside its data directory.
pub const ML_DIR: &str = "ml";

/// The installed app's data directory, or `None` when the OS has no home. Never creates or moves
/// anything: until older Windows data leaves the roaming folder, that is the answer
/// (`data_location::choose`).
pub fn app_data_dir() -> Option<PathBuf> {
    let local = local_data_dir()?;
    Some(match legacy_data_dir() {
        Some(legacy) => crate::data_location::choose(&legacy, &local),
        None => local,
    })
}

pub fn local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_IDENTIFIER))
}

/// The model file a `local_model` setting names, or `None` for anything but a plain file name
/// (a setting must never reach outside the models folder).
pub fn model_file(models_dir: &Path, name: &str) -> Option<PathBuf> {
    let mut parts = Path::new(name).components();
    let plain = matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(part)), None) if part == name
    );
    plain.then(|| models_dir.join(name))
}

/// The personal model a `model_artifacts` row names, or `None` (logged) unless it resolves to a
/// `.mlmodelc` directly inside `ml_dir`: a row must never choose what gets deleted or loaded.
pub fn model_artifact(ml_dir: &Path, stored: &str) -> Option<PathBuf> {
    let artifact = Path::new(stored)
        .canonicalize()
        .ok()
        .zip(ml_dir.canonicalize().ok())
        .filter(|(path, ml_dir)| {
            path.parent() == Some(ml_dir) && path.extension().is_some_and(|ext| ext == "mlmodelc")
        })
        .map(|(path, _)| path);
    if artifact.is_none() {
        eprintln!("ignoring a model path that is not a model in {ml_dir:?}: {stored}");
    }
    artifact
}

/// The roaming folder earlier Windows builds kept the data in (Tauri's
/// `app_data_dir`). `None` elsewhere, where there was nothing to move.
pub fn legacy_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::data_dir().map(|dir| dir.join(APP_IDENTIFIER))
    } else {
        None
    }
}

pub fn app_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join(APP_IDENTIFIER))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_setting_must_be_a_plain_file_name() {
        let dir = Path::new("models");
        assert_eq!(model_file(dir, "tiny.gguf"), Some(dir.join("tiny.gguf")));
        for name in ["", ".", "..", "../x.gguf", "a/b.gguf", "/etc/passwd"] {
            assert_eq!(model_file(dir, name), None, "{name}");
        }
    }

    #[test]
    fn a_stored_model_path_must_be_a_model_directly_inside_the_ml_folder() {
        let root = std::env::temp_dir().join(format!("openrize-paths-{}", std::process::id()));
        let ml = root.join(ML_DIR);
        let model = ml.join("category-1.mlmodelc");
        let nested = ml.join("deeper").join("project-1.mlmodelc");
        let outside = root.join("victim.mlmodelc");
        for dir in [&model, &nested, &outside, &ml.join("notes")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let stored = |path: &Path| path.to_string_lossy().to_string();

        assert_eq!(
            model_artifact(&ml, &stored(&model)),
            Some(model.canonicalize().unwrap())
        );
        let rejected = [
            stored(&root),
            stored(&ml),
            stored(&outside),
            stored(&nested),
            stored(&ml.join("notes")),
            stored(&ml.join("missing.mlmodelc")),
            stored(&ml.join("..").join("victim.mlmodelc")),
            "category-1.mlmodelc".to_string(),
            String::new(),
        ];
        for path in &rejected {
            assert_eq!(model_artifact(&ml, path), None, "{path}");
        }
        #[cfg(unix)]
        {
            let link = ml.join("link.mlmodelc");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert_eq!(model_artifact(&ml, &stored(&link)), None);
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
