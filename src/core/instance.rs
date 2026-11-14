//! Instances: a game (or server) folder with its own version + mod loader.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use super::accounts::Account;
use super::launch::{self, ClientOpts};
use super::loaders::{self, Loader, ServerLaunch};
use super::paths::paths;
use super::rules::Env;
use super::session::{Reporter, RunState};
use super::settings::{self, Settings};
use super::{java, versions};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Client,
    Server,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ServerCfg {
    pub port: u16,
    pub online_mode: bool,
    pub motd: String,
    pub max_players: u32,
    pub eula: bool,
    pub launch: Option<ServerLaunch>,
}

impl Default for ServerCfg {
    fn default() -> Self {
        ServerCfg {
            port: 25565,
            online_mode: false,
            motd: "A Lodestone server".into(),
            max_players: 20,
            eula: false,
            launch: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub mc_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    /// Installed launch profile id (e.g. `fabric-loader-0.20.1-26.2`).
    pub version_id: Option<String>,
    pub created: i64,
    pub last_played: Option<i64>,
    pub memory_mb: Option<u32>,
    pub jvm_args: String,
    pub server: ServerCfg,
    /// Modpack this instance was created from, if any.
    pub modpack: Option<String>,
    /// Custom icon file name inside the instance folder.
    pub icon: Option<String>,
}

fn slugify(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "instance".into() } else { s }
}

impl Instance {
    pub fn new(name: &str, kind: Kind, mc: &str, loader: Loader, lv: Option<String>) -> Instance {
        let base = slugify(name);
        let mut id = base.clone();
        let mut n = 2;
        while paths().instances().join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        Instance {
            id,
            name: name.to_string(),
            kind,
            mc_version: mc.to_string(),
            loader,
            loader_version: lv,
            created: chrono::Utc::now().timestamp(),
            ..Default::default()
        }
    }

    pub fn dir(&self) -> PathBuf {
        paths().instances().join(&self.id)
    }

    pub fn game_dir(&self) -> PathBuf {
        match self.kind {
            Kind::Client => self.dir().join("minecraft"),
            Kind::Server => self.dir().join("server"),
        }
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(self.game_dir())?;
        std::fs::write(self.dir().join("instance.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn load(id: &str) -> Option<Instance> {
        let b = std::fs::read(paths().instances().join(id).join("instance.json")).ok()?;
        serde_json::from_slice(&b).ok()
    }

    pub fn load_all() -> Vec<Instance> {
        let mut out: Vec<Instance> = std::fs::read_dir(paths().instances())
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| Instance::load(&e.file_name().to_string_lossy()))
            .collect();
        out.sort_by_key(|i| std::cmp::Reverse(i.last_played.unwrap_or(i.created)));
        out
    }

    pub fn delete(&self) -> Result<()> {
        std::fs::remove_dir_all(self.dir())?;
        Ok(())
    }

    pub fn loader_label(&self) -> String {
        match (&self.loader, &self.loader_version) {
            (Loader::Vanilla, _) => "Vanilla".into(),
            (l, Some(v)) => format!("{} {}", l.name(), short_loader_version(v)),
            (l, None) => l.name().into(),
        }
    }

    pub fn content_dir(&self, sub: &str) -> PathBuf {
        self.game_dir().join(sub)
    }

    /// Files in `mods/`, `resourcepacks/`, `shaderpacks/`.
    pub fn list_content(&self, sub: &str) -> Vec<ContentFile> {
        let mut out: Vec<ContentFile> = std::fs::read_dir(self.content_dir(sub))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                ContentFile {
                    enabled: !name.ends_with(".disabled"),
                    display: name.trim_end_matches(".disabled").to_string(),
                    size: e.metadata().map(|m| m.len()).unwrap_or(0),
                    path: e.path(),
                }
            })
            .filter(|f| !f.display.starts_with('.'))
            .collect();
        out.sort_by_key(|f| f.display.to_lowercase());
        out
    }

    /// Copy files into `mods/`, `resourcepacks/` or `shaderpacks/`.
    /// Mods must be `.jar`; packs must be `.zip`. Returns (added, skipped) names.
    pub fn import_content(&self, sub: &str, paths: &[PathBuf]) -> Result<(Vec<String>, Vec<String>)> {
        import_into(&self.content_dir(sub), sub, paths)
    }

    /// Path of the custom icon, if one is set and still exists.
    pub fn icon_path(&self) -> Option<PathBuf> {
        let p = self.dir().join(self.icon.as_ref()?);
        p.exists().then_some(p)
    }

    /// Use an image file as this instance's icon (PNG, JPG, WebP or GIF).
    pub fn set_icon_from(&mut self, src: &Path) -> Result<()> {
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .filter(|e| ["png", "jpg", "jpeg", "webp", "gif"].contains(&e.as_str()))
            .ok_or_else(|| anyhow!("Pick a PNG, JPG, WebP or GIF image"))?;
        std::fs::create_dir_all(self.dir())?;
        // A new file name each time, so the UI doesn't show a cached old image.
        let name = format!("icon-{}.{ext}", chrono::Utc::now().timestamp_millis());
        std::fs::copy(src, self.dir().join(&name))?;
        self.clear_icon_file();
        self.icon = Some(name);
        self.save()
    }

    pub fn reset_icon(&mut self) -> Result<()> {
        self.clear_icon_file();
        self.icon = None;
        self.save()
    }

    fn clear_icon_file(&self) {
        if let Some(old) = &self.icon {
            let _ = std::fs::remove_file(self.dir().join(old));
        }
    }

    pub fn memory(&self, s: &Settings) -> u32 {
        self.memory_mb.unwrap_or(s.max_memory_mb)
    }
}

pub fn accepted_extensions(sub: &str) -> &'static [&'static str] {
    match sub {
        "mods" => &["jar"],
        _ => &["zip"],
    }
}

fn import_into(dir: &Path, sub: &str, paths: &[PathBuf]) -> Result<(Vec<String>, Vec<String>)> {
    let exts = accepted_extensions(sub);
    let (mut added, mut skipped) = (Vec::new(), Vec::new());
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let ok = p.is_file() && p.extension().map(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x))).unwrap_or(false);
        if !ok {
            skipped.push(name);
            continue;
        }
        std::fs::create_dir_all(dir)?;
        std::fs::copy(p, dir.join(&name)).map_err(|e| anyhow!("couldn't copy {name}: {e}"))?;
        added.push(name);
    }
    Ok((added, skipped))
}

pub fn short_loader_version(v: &str) -> String {
    // "1.20.1-47.3.0" -> "47.3.0"
    match v.split_once('-') {
        Some((mc, rest)) if mc.starts_with("1.") || mc.starts_with("26") => rest.to_string(),
        _ => v.to_string(),
    }
}

#[derive(Clone, Debug)]
pub struct ContentFile {
    pub display: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub size: u64,
}

impl ContentFile {
    pub fn toggle(&self) -> Result<()> {
        let new = if self.enabled {
            PathBuf::from(format!("{}.disabled", self.path.to_string_lossy()))
        } else {
            PathBuf::from(self.path.to_string_lossy().trim_end_matches(".disabled").to_string())
        };
        std::fs::rename(&self.path, new)?;
        Ok(())
    }
}

// ------------------------------------------------------------------ client

/// Install everything for a client instance; returns resolved version + Java.
async fn prepare_client(inst: &mut Instance, s: &Settings, rep: &Reporter) -> Result<(versions::Resolved, PathBuf, Env)> {
    let needs_profile = match &inst.version_id {
        None => true,
        Some(id) => !paths().version_json(id).exists(),
    };
    if needs_profile {
        let lv = inst.loader_version.clone().unwrap_or_default();
        if inst.loader != Loader::Vanilla && lv.is_empty() {
            return Err(anyhow!("no {} version selected", inst.loader.name()));
        }
        rep.stage("Fetching version info");
        versions::ensure_json(&inst.mc_version).await?;
        let id = loaders::install_client(inst.loader, &inst.mc_version, &lv, rep).await?;
        inst.version_id = Some(id);
        inst.save()?;
    }
    let res = versions::resolve(inst.version_id.as_deref().unwrap()).await?;
    let mut env = Env::host();
    let plan = java::plan(&res.json, &env).await?;
    env.arch = plan.arch.clone();
    versions::install_client_files(&res, &env, rep).await?;
    let java = java::ensure(&plan, s.java_override.as_deref(), rep).await?;
    Ok((res, java, env))
}

pub async fn play(mut inst: Instance, s: Settings, account: Option<Account>, rep: Reporter) -> Result<()> {
    rep.set_state(RunState::Working);
    let username = account.as_ref().map(|a| a.username.clone()).unwrap_or_else(|| "Player".into());
    let (res, java, env) = prepare_client(&mut inst, &s, &rep).await?;
    let mut extra = settings::split_args(&s.extra_jvm_args);
    extra.extend(settings::split_args(&inst.jvm_args));
    let opts = ClientOpts {
        username,
        game_dir: inst.game_dir(),
        min_mb: s.min_memory_mb.min(inst.memory(&s)),
        max_mb: inst.memory(&s),
        extra_jvm: extra,
        resolution: s.width.zip(s.height),
    };
    let spec = launch::build_client(&res, &java, &env, &opts)?;
    inst.last_played = Some(chrono::Utc::now().timestamp());
    inst.save()?;
    rep.stage(format!("Starting Minecraft {}", res.json.id));
    let code = launch::run(spec, rep.clone(), false).await?;
    rep.info(format!("Game exited with code {code}"));
    if code != 0 && code != 130 {
        rep.warn("Minecraft closed with an error. Check the log above for details.");
    }
    Ok(())
}

// ------------------------------------------------------------------ server

pub fn write_server_files(inst: &Instance) -> Result<()> {
    let dir = inst.game_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("eula.txt"),
        format!(
            "# Accepted in Lodestone. See https://aka.ms/MinecraftEULA\neula={}\n",
            inst.server.eula
        ),
    )?;
    let cfg = &inst.server;
    set_properties(
        &dir.join("server.properties"),
        &[
            ("server-port", cfg.port.to_string()),
            ("online-mode", cfg.online_mode.to_string()),
            ("motd", cfg.motd.clone()),
            ("max-players", cfg.max_players.to_string()),
        ],
    )?;
    Ok(())
}

/// Update keys in a .properties file, keeping everything else.
pub fn set_properties(path: &Path, values: &[(&str, String)]) -> Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
    for (k, v) in values {
        let line = format!("{k}={}", escape_prop(v));
        match lines.iter().position(|l| l.split_once('=').map(|(a, _)| a.trim() == *k).unwrap_or(false)) {
            Some(i) => lines[i] = line,
            None => lines.push(line),
        }
    }
    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

fn escape_prop(v: &str) -> String {
    v.chars()
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            c if (c as u32) > 127 => format!("\\u{:04x}", c as u32).chars().collect(),
            c => vec![c],
        })
        .collect()
}

pub fn read_properties(path: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())))
        .collect()
}

pub async fn start_server(mut inst: Instance, s: Settings, rep: Reporter) -> Result<()> {
    rep.set_state(RunState::Working);
    if !inst.server.eula {
        return Err(anyhow!("Accept the Minecraft EULA in the server settings first"));
    }
    if inst.server.launch.is_none() {
        let launch = loaders::install_server(
            inst.loader,
            &inst.mc_version,
            inst.loader_version.as_deref(),
            &inst.game_dir(),
            &rep,
        )
        .await?;
        inst.server.launch = Some(launch);
        inst.save()?;
    }
    write_server_files(&inst)?;
    let (java, _) = java::for_mc_version(&inst.mc_version, s.java_override.as_deref(), &rep).await?;
    let mem = inst.memory(&s);
    let mut args = vec![format!("-Xms{}M", mem.min(1024)), format!("-Xmx{mem}M")];
    args.extend(settings::split_args(&inst.jvm_args));
    args.extend(inst.server.launch.clone().unwrap().args);
    inst.last_played = Some(chrono::Utc::now().timestamp());
    inst.save()?;
    rep.stage(format!("Starting {} server on port {}", inst.loader.name(), inst.server.port));
    let code = launch::run(launch::LaunchSpec { java, args, cwd: inst.game_dir() }, rep.clone(), true).await?;
    rep.info(format!("Server stopped (exit code {code})"));
    Ok(())
}

/// LAN address to show for "connect to".
pub fn lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("1.1.1.1:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_only_matching_files() {
        let tmp = std::env::temp_dir().join(format!("lodestone-import-{}", std::process::id()));
        let src = tmp.join("src");
        std::fs::create_dir_all(&src).unwrap();
        for f in ["sodium.jar", "Pack.ZIP", "notes.txt"] {
            std::fs::write(src.join(f), b"x").unwrap();
        }
        let files: Vec<PathBuf> = ["sodium.jar", "Pack.ZIP", "notes.txt", "missing.jar"].iter().map(|f| src.join(f)).collect();
        let (added, skipped) = import_into(&tmp.join("mods"), "mods", &files).unwrap();
        assert_eq!(added, vec!["sodium.jar"]);
        assert_eq!(skipped.len(), 3);
        assert!(tmp.join("mods/sodium.jar").exists());
        let (added, _) = import_into(&tmp.join("resourcepacks"), "resourcepacks", &files).unwrap();
        assert_eq!(added, vec!["Pack.ZIP"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
