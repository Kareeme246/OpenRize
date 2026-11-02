//! macOS: `~/.local/bin/rize` is a symlink to the `rize` bundled next to the app
//! executable. It is replaced on every launch, so a removed link heals itself.

pub fn install_bundled_cli() {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let bundled = std::env::current_exe()?.with_file_name("rize");
        if !bundled.is_file() {
            return Ok(());
        }
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let directory = std::path::PathBuf::from(home).join(".local/bin");
        std::fs::create_dir_all(&directory)?;
        let destination = directory.join("rize");
        let temporary = directory.join(format!(".rize-{}", std::process::id()));
        let _ = std::fs::remove_file(&temporary);
        std::os::unix::fs::symlink(&bundled, &temporary)?;
        std::fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("could not update ~/.local/bin/rize: {error}");
    }
}
