//! Self-install: make sure Lodestone lives in the per-user programs folder
//! (no admin needed) and is findable by the OS search.
//!
//! - Windows: `%LOCALAPPDATA%\Programs\Lodestone\Lodestone.exe`, a Start Menu
//!   shortcut (Windows Search) and an "Apps & features" uninstall entry, all
//!   per-user (HKCU), the same place the installer uses.
//! - macOS: `~/Applications/Lodestone.app` (Spotlight indexes it; no admin).
//!   An app already in `/Applications` is left where it is.
//! - Linux: `~/.local/opt/lodestone/lodestone`, plus an app-menu entry in
//!   `~/.local/share/applications/lodestone.desktop` with the icon.
//!
//! User data lives in the OS app-data folder, so moving the app never touches
//! instances, worlds, accounts or settings.

use std::path::{Path, PathBuf};

#[allow(unused_imports)]
use anyhow::{Result, anyhow};

/// Argument passed to the relocated copy so it can remove the old one.
pub const RELOCATED_FLAG: &str = "--relocated-from";

/// Where Lodestone should be installed for this user, if the platform has one.
pub fn install_location() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("Programs").join("Lodestone").join("Lodestone.exe"))
    } else if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Applications").join("Lodestone.app"))
    } else if cfg!(target_os = "linux") {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(".local").join("opt").join("lodestone").join("lodestone"))
    } else {
        None
    }
}

fn normalize(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\");
    let s = s.trim_start_matches(r"\\?\").to_string();
    if cfg!(windows) { s.to_lowercase() } else { p.to_string_lossy().to_string() }
}

/// Is the running app already somewhere it should stay?
fn already_installed(current: &Path, target: &Path) -> bool {
    if normalize(current) == normalize(target) {
        return true;
    }
    if cfg!(target_os = "macos") {
        // Respect a copy the user put in /Applications.
        let s = current.to_string_lossy();
        if s.starts_with("/Applications/") {
            return true;
        }
    }
    false
}

/// If Lodestone isn't in the programs folder yet, copy it there, register it,
/// start the installed copy and return `true` (the caller should exit).
pub fn ensure_installed() -> Result<bool> {
    if std::env::var_os("LODESTONE_NO_SELF_INSTALL").is_some() || cfg!(debug_assertions) {
        return Ok(false);
    }
    let Some(target) = install_location() else { return Ok(false) };
    let current = super::update::install_target()?;
    if already_installed(&current, &target) {
        // Restore a missing app-menu entry (e.g. unpacked by hand into the install folder).
        #[cfg(target_os = "linux")]
        if !linux::desktop_file().map(|p| p.exists()).unwrap_or(true) {
            let _ = register(&target);
        }
        return Ok(false);
    }
    install_to(&current, &target)?;
    register(&target)?;
    relaunch_installed(&target, &current)?;
    Ok(true)
}

#[cfg(target_os = "windows")]
fn install_to(current: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target.parent().unwrap())?;
    let staged = target.with_extension("exe.new");
    std::fs::copy(current, &staged)?;
    let old = target.with_extension("exe.old");
    if target.exists() {
        let _ = std::fs::remove_file(&old);
        std::fs::rename(target, &old)?;
    }
    std::fs::rename(&staged, target)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_to(current: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target.parent().unwrap())?;
    let staged = target.with_extension("app.new");
    let _ = std::fs::remove_dir_all(&staged);
    let ok = std::process::Command::new("ditto").arg(current).arg(&staged).status()?.success();
    if !ok {
        return Err(anyhow!("couldn't copy Lodestone to {}", target.display()));
    }
    let _ = std::process::Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&staged).status();
    if target.exists() {
        let _ = std::fs::remove_dir_all(target);
    }
    std::fs::rename(&staged, target)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_to(current: &Path, target: &Path) -> Result<()> {
    linux::copy_executable(current, target)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn install_to(_current: &Path, _target: &Path) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    pub const ICON: &[u8] = include_bytes!("../../assets/icon.png");

    pub fn desktop_file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
        Some(base.join("applications").join("lodestone.desktop"))
    }

    /// Copy the running binary to `target` (staged, then renamed), executable.
    pub fn copy_executable(current: &Path, target: &Path) -> Result<()> {
        std::fs::create_dir_all(target.parent().unwrap())?;
        let staged = target.with_extension("new");
        std::fs::copy(current, &staged)?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&staged, target)?;
        Ok(())
    }

    /// The `.desktop` entry that puts Lodestone in the app menu.
    pub fn desktop_entry(exe: &Path, icon: &Path) -> String {
        let q = |p: &Path| p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "[Desktop Entry]\nType=Application\nName=Lodestone\nGenericName=Minecraft Launcher\nComment=Play Minecraft with any version and mod loader\nExec=\"{}\"\nIcon={}\nTerminal=false\nCategories=Game;\nKeywords=minecraft;launcher;mods;\nStartupWMClass=dev.p1e2r.lodestone\n",
            q(exe),
            icon.to_string_lossy()
        )
    }

    /// Write the icon next to the app and the app-menu entry.
    pub fn write_menu_entry(exe: &Path, desktop: &Path) -> Result<()> {
        let icon = exe.parent().unwrap().join("lodestone.png");
        std::fs::write(&icon, ICON)?;
        std::fs::create_dir_all(desktop.parent().unwrap())?;
        std::fs::write(desktop, desktop_entry(exe, &icon))?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
}

/// Start Menu shortcut + per-user uninstall entry (no admin).
#[cfg(target_os = "windows")]
fn register(target: &Path) -> Result<()> {
    let exe = target.to_string_lossy().to_string();
    let dir = target.parent().unwrap().to_string_lossy().to_string();
    let ps_exe = exe.replace('\'', "''");
    let ps_dir = dir.replace('\'', "''");
    let script = format!(
        "$p = Join-Path $env:APPDATA 'Microsoft\\Windows\\Start Menu\\Programs\\Lodestone.lnk'; \
         $s = (New-Object -ComObject WScript.Shell).CreateShortcut($p); \
         $s.TargetPath = '{ps_exe}'; $s.WorkingDirectory = '{ps_dir}'; $s.IconLocation = '{ps_exe},0'; \
         $s.Description = 'Lodestone Minecraft launcher'; $s.Save()"
    );
    let _ = hidden(std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script])).status();

    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Lodestone";
    let values: [(&str, &str, String); 8] = [
        ("DisplayName", "REG_SZ", "Lodestone".into()),
        ("DisplayVersion", "REG_SZ", env!("CARGO_PKG_VERSION").into()),
        ("Publisher", "REG_SZ", "p1e2r".into()),
        ("DisplayIcon", "REG_SZ", format!("{exe},0")),
        ("InstallLocation", "REG_SZ", dir.clone()),
        ("UninstallString", "REG_SZ", format!("\"{exe}\" --uninstall")),
        ("NoModify", "REG_DWORD", "1".into()),
        ("NoRepair", "REG_DWORD", "1".into()),
    ];
    for (name, kind, data) in values {
        let _ = hidden(std::process::Command::new("reg").args(["add", key, "/v", name, "/t", kind, "/d", &data, "/f"])).status();
    }
    Ok(())
}

/// App-menu entry with the icon (no admin).
#[cfg(target_os = "linux")]
fn register(target: &Path) -> Result<()> {
    let desktop = linux::desktop_file().ok_or_else(|| anyhow!("no home folder"))?;
    linux::write_menu_entry(target, &desktop)?;
    if let Some(dir) = desktop.parent() {
        let _ = std::process::Command::new("update-desktop-database").arg(dir).status();
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn register(_target: &Path) -> Result<()> {
    Ok(())
}

fn relaunch_installed(target: &Path, old: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-n")
            .arg(target)
            .arg("--args")
            .arg(RELOCATED_FLAG)
            .arg(old)
            .spawn()?;
    } else {
        std::process::Command::new(target).arg(RELOCATED_FLAG).arg(old).spawn()?;
    }
    Ok(())
}

/// Called by the relocated copy: remove the copy we were moved from, once it has exited.
pub fn remove_old_copy(old: PathBuf) {
    std::thread::spawn(move || {
        // Never delete from a read-only disk image or a translocated path.
        let s = old.to_string_lossy();
        if s.starts_with("/Volumes/") || s.contains("AppTranslocation") {
            return;
        }
        if let Some(target) = install_location() {
            if normalize(&old) == normalize(&target) {
                return;
            }
        }
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let r = if old.is_dir() { std::fs::remove_dir_all(&old) } else { std::fs::remove_file(&old) };
            if r.is_ok() || !old.exists() {
                return;
            }
        }
    });
}

/// `Lodestone.exe --uninstall` (Windows "Apps & features"): remove the app,
/// its Start Menu shortcut and uninstall entry. User data is kept.
#[cfg(target_os = "windows")]
pub fn uninstall() -> Result<()> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let _ = std::fs::remove_file(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Lodestone.lnk"));
    }
    let _ = hidden(std::process::Command::new("reg").args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Lodestone", "/f"])).status();
    if let Some(target) = install_location() {
        // The running exe can't delete itself; let cmd do it a moment after we exit.
        let dir = target.parent().unwrap().to_string_lossy().to_string();
        let _ = hidden(std::process::Command::new("cmd").args(["/C", &format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{dir}\"")])).spawn();
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn uninstall() -> Result<()> {
    Err(anyhow!("--uninstall is only used on Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn location_is_per_user() {
        if let Some(p) = install_location() {
            let s = p.to_string_lossy();
            assert!(s.contains("Programs") || s.contains("Applications") || s.contains(".local/opt"));
        }
        assert!(already_installed(Path::new("/a/b"), Path::new("/a/b")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_self_install() {
        let tmp = std::env::temp_dir().join(format!("lodestone-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let src = tmp.join("Downloads").join("lodestone");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"binary").unwrap();
        let target = tmp.join("home").join(".local/opt/lodestone/lodestone");
        install_to(&src, &target).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        let desktop = tmp.join("home/.local/share/applications/lodestone.desktop");
        linux::write_menu_entry(&target, &desktop).unwrap();
        let entry = std::fs::read_to_string(&desktop).unwrap();
        assert!(entry.contains(&format!("Exec=\"{}\"", target.display())));
        assert!(entry.contains("Icon=") && target.parent().unwrap().join("lodestone.png").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
