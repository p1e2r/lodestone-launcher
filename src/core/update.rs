//! One-click updates from GitHub releases.
//!
//! Checks `releases/latest` of this repository, picks the asset for this
//! platform, downloads it, swaps the running app/binary for the new one and
//! relaunches.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use super::loaders::cmp_versions;
use super::net::{self, Dl};
use super::paths::paths;
use super::session::Reporter;

pub const REPO: &str = "p1e2r/lodestone-launcher";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize, Clone, Debug)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

impl Release {
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }
    pub fn asset(&self) -> Option<&Asset> {
        let want = asset_name()?;
        self.assets.iter().find(|a| a.name == want)
    }
}

/// The release file built for this platform.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(if cfg!(target_arch = "aarch64") { "Lodestone-macos-arm64.zip" } else { "Lodestone-macos-intel.zip" })
    } else if cfg!(target_os = "windows") {
        Some("Lodestone-windows-x64.exe")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some("Lodestone-linux-x64.tar.gz")
    } else {
        None
    }
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    cmp_versions(candidate.trim_start_matches('v'), current.trim_start_matches('v')) == std::cmp::Ordering::Greater
}

/// The latest release, if it's newer than this build and has a file for this platform.
pub async fn check() -> Result<Option<Release>> {
    let rel: Release = net::get_json(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .await
        .context("couldn't check GitHub for updates")?;
    if rel.draft || rel.prerelease || !is_newer(rel.version(), CURRENT) || rel.asset().is_none() {
        return Ok(None);
    }
    Ok(Some(rel))
}

/// Where the running app lives: the `.app` bundle on macOS, the executable elsewhere.
pub fn install_target() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if cfg!(target_os = "macos") {
        exe.ancestors()
            .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Automatic updates only work when Lodestone runs from Lodestone.app"))
    } else {
        Ok(exe)
    }
}

/// Download and install `rel` in place of the running app. Call [`relaunch`] afterwards.
pub async fn install(rel: &Release, rep: &Reporter) -> Result<PathBuf> {
    let asset = rel.asset().ok_or_else(|| anyhow!("this release has no build for your system"))?.clone();
    let target = install_target()?;
    let work = paths().cache().join("update").join(rel.version());
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;

    rep.stage(format!("Downloading Lodestone {}", rel.version()));
    let file = work.join(&asset.name);
    net::download_one(&Dl::new(&asset.browser_download_url, &file).size(Some(asset.size))).await?;

    rep.stage("Installing update");
    let target2 = target.clone();
    tokio::task::spawn_blocking(move || replace(&file, &work, &target2)).await??;
    Ok(target)
}

#[cfg(target_os = "macos")]
fn replace(file: &Path, work: &Path, target: &Path) -> Result<()> {
    let extract = work.join("extract");
    std::fs::create_dir_all(&extract)?;
    let ok = std::process::Command::new("ditto").args(["-x", "-k"]).arg(file).arg(&extract).status()?.success();
    if !ok {
        bail!("couldn't unzip the update");
    }
    let new_app = extract.join("Lodestone.app");
    if !new_app.join("Contents/MacOS/Lodestone").exists() {
        bail!("the downloaded update doesn't contain Lodestone.app");
    }
    // Files we download ourselves aren't quarantined, but make sure.
    let _ = std::process::Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&new_app).status();

    let backup = target.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(target, &backup).context("couldn't move the old app aside (is the folder writable?)")?;
    let moved = std::fs::rename(&new_app, target).or_else(|_| {
        // Different volume: copy instead.
        let ok = std::process::Command::new("ditto").arg(&new_app).arg(target).status()?.success();
        if ok { Ok(()) } else { Err(std::io::Error::other("copy failed")) }
    });
    if let Err(e) = moved {
        let _ = std::fs::rename(&backup, target);
        bail!("couldn't install the update: {e}");
    }
    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
}

#[cfg(target_os = "windows")]
fn replace(file: &Path, _work: &Path, target: &Path) -> Result<()> {
    // A running .exe can't be overwritten, but it can be renamed.
    let old = target.with_extension("exe.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old).context("couldn't move the old version aside")?;
    if let Err(e) = std::fs::copy(file, target) {
        let _ = std::fs::rename(&old, target);
        bail!("couldn't install the update: {e}");
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn replace(file: &Path, work: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let ok = std::process::Command::new("tar").arg("-xzf").arg(file).arg("-C").arg(work).status()?.success();
    let new_bin = work.join("lodestone");
    if !ok || !new_bin.exists() {
        bail!("couldn't unpack the update");
    }
    std::fs::set_permissions(&new_bin, std::fs::Permissions::from_mode(0o755))?;
    // Renaming over a running binary is fine on Linux; copy first so it's on the same filesystem.
    let staged = target.with_extension("new");
    std::fs::copy(&new_bin, &staged).context("couldn't write next to the current binary (is the folder writable?)")?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, target)?;
    Ok(())
}

/// Start the freshly installed version. The caller should quit right after.
pub fn relaunch(target: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-n").arg(target).spawn()?;
    } else {
        std::process::Command::new(target).spawn()?;
    }
    Ok(())
}

/// Remove leftovers from a previous update (Windows keeps the old .exe until restart).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
    let _ = std::fs::remove_dir_all(paths().cache().join("update"));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newer() {
        assert!(is_newer("v0.1.2", "0.1.1"));
        assert!(is_newer("0.2.0", "0.1.10"));
        assert!(!is_newer("v0.1.1", "0.1.1"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        assert!(asset_name().is_some());
    }
}
