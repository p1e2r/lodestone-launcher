//! Builds the `java ...` command line for a resolved version and runs
//! processes (game clients and servers) with live log capture.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use md5::{Digest, Md5};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Notify;

use super::model::Arg;
use super::paths::paths;
use super::rules::{self, Env};
use super::session::{Level, Reporter, RunState};
use super::versions::{self, Resolved};

pub const CP_SEP: &str = if cfg!(windows) { ";" } else { ":" };

/// UUID that vanilla servers assign to an offline player:
/// `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`.
pub fn offline_uuid(name: &str) -> String {
    let mut b: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    hex::encode(b)
}

pub fn dashed_uuid(u: &str) -> String {
    if u.len() != 32 {
        return u.to_string();
    }
    format!("{}-{}-{}-{}-{}", &u[0..8], &u[8..12], &u[12..16], &u[16..20], &u[20..32])
}

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ClientOpts {
    pub username: String,
    pub game_dir: PathBuf,
    pub min_mb: u32,
    pub max_mb: u32,
    pub extra_jvm: Vec<String>,
    pub resolution: Option<(u32, u32)>,
}

fn substitute(s: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = s.to_string();
    if !out.contains("${") {
        return out;
    }
    for (k, v) in vars {
        let pat = format!("${{{k}}}");
        if out.contains(&pat) {
            out = out.replace(&pat, v);
        }
    }
    out
}

fn expand(args: &[Arg], env: &Env, vars: &HashMap<&str, String>) -> Vec<String> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Arg::Plain(s) => out.push(substitute(s, vars)),
            Arg::Ruled { rules, value } => {
                if rules::rules_allow(&Some(rules.clone()), env) {
                    out.extend(value.to_vec().iter().map(|s| substitute(s, vars)));
                }
            }
        }
    }
    out
}

/// Extract legacy natives jars into the version's natives directory.
fn extract_natives(res: &Resolved, env: &Env, dir: &Path) -> Result<()> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    for lib in &res.json.libraries {
        if !rules::rules_allow(&lib.rules, env) {
            continue;
        }
        let Some(n) = rules::native_artifact(lib, env) else { continue };
        let jar = paths().libraries().join(&n.rel);
        let Ok(file) = std::fs::File::open(&jar) else { continue };
        let mut zip = zip::ZipArchive::new(file)?;
        let mut excludes: Vec<String> = lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default();
        excludes.push("META-INF/".into());
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name()?.to_string();
            if entry.is_dir() || excludes.iter().any(|e| name.starts_with(e)) {
                continue;
            }
            let Some(safe) = entry.enclosed_name() else { continue };
            let out = dir.join(safe);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
        }
    }
    Ok(())
}

pub fn build_client(res: &Resolved, java: &Path, env: &Env, opts: &ClientOpts) -> Result<LaunchSpec> {
    let p = paths();
    let json = &res.json;
    let natives_dir = p.version_dir(&json.id).join("natives");
    extract_natives(res, env, &natives_dir)?;
    std::fs::create_dir_all(&opts.game_dir)?;
    versions::copy_resources_if_needed(res, &opts.game_dir)?;

    // Classpath: libraries (child profile first wins), then the game jar.
    let mut seen = HashSet::new();
    let mut cp: Vec<String> = Vec::new();
    for lib in &json.libraries {
        if !rules::library_wanted(lib, env) {
            continue;
        }
        let Some(a) = rules::main_artifact(lib) else { continue };
        if !seen.insert(rules::lib_key(&lib.name)) {
            continue;
        }
        let path = p.libraries().join(&a.rel);
        let s = path.to_string_lossy().to_string();
        if !cp.contains(&s) {
            cp.push(s);
        }
    }
    cp.push(p.version_jar(&res.jar_id).to_string_lossy().to_string());

    let asset_index = json.asset_index.as_ref().map(|a| a.id.clone()).or_else(|| json.assets.clone()).unwrap_or_else(|| "legacy".into());
    let assets_root = p.assets();
    let virtual_dir = versions::virtual_assets_dir(&asset_index);
    let game_assets = if virtual_dir.exists() { virtual_dir } else { assets_root.clone() };
    let uuid = offline_uuid(&opts.username);
    let (w, h) = opts.resolution.unwrap_or((854, 480));

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", opts.username.clone());
    vars.insert("version_name", json.id.clone());
    vars.insert("game_directory", opts.game_dir.to_string_lossy().into());
    vars.insert("assets_root", assets_root.to_string_lossy().into());
    vars.insert("game_assets", game_assets.to_string_lossy().into());
    vars.insert("assets_index_name", asset_index);
    vars.insert("auth_uuid", uuid);
    vars.insert("auth_access_token", "0".into());
    vars.insert("auth_session", "token:0".into());
    vars.insert("clientid", String::new());
    vars.insert("auth_xuid", "0".into());
    vars.insert("user_type", "legacy".into());
    vars.insert("user_properties", "{}".into());
    vars.insert("version_type", json.kind.clone().unwrap_or_else(|| "release".into()));
    vars.insert("natives_directory", natives_dir.to_string_lossy().into());
    vars.insert("launcher_name", "Lodestone".into());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath", cp.join(CP_SEP));
    vars.insert("classpath_separator", CP_SEP.into());
    vars.insert("library_directory", p.libraries().to_string_lossy().into());
    vars.insert("resolution_width", w.to_string());
    vars.insert("resolution_height", h.to_string());

    let mut env = env.clone();
    if opts.resolution.is_some() {
        env.features.insert("has_custom_resolution".into(), true);
    }

    let mut args = vec![format!("-Xms{}M", opts.min_mb), format!("-Xmx{}M", opts.max_mb)];
    args.extend(opts.extra_jvm.iter().cloned());
    if let Some(log) = json.logging.as_ref().and_then(|l| l.client.as_ref()) {
        let path = p.assets().join("log_configs").join(&log.file.id);
        if path.exists() {
            args.push(log.argument.replace("${path}", &path.to_string_lossy()));
        }
    }
    match json.arguments.as_ref().filter(|a| !a.jvm.is_empty()) {
        Some(a) => args.extend(expand(&a.jvm, &env, &vars)),
        None => {
            if env.os == "osx" {
                args.push("-Xdock:name=Minecraft".into());
            }
            args.push(format!("-Djava.library.path={}", natives_dir.to_string_lossy()));
            args.push("-cp".into());
            args.push(cp.join(CP_SEP));
        }
    }
    args.push(json.main_class.clone().ok_or_else(|| anyhow!("version has no main class"))?);
    if let Some(a) = &json.arguments {
        args.extend(expand(&a.game, &env, &vars));
    }
    if let Some(legacy) = &json.minecraft_arguments {
        args.extend(legacy.split_whitespace().map(|s| substitute(s, &vars)));
        if opts.resolution.is_some() {
            args.extend(["--width".into(), w.to_string(), "--height".into(), h.to_string()]);
        }
    }

    Ok(LaunchSpec { java: java.to_path_buf(), args, cwd: opts.game_dir.clone() })
}

/// Turns Log4j's XML event stream into readable lines.
#[derive(Default)]
struct XmlLog {
    in_event: bool,
    level: String,
    thread: String,
    text: String,
    in_cdata: bool,
}

fn attr(line: &str, name: &str) -> String {
    let pat = format!("{name}=\"");
    line.find(&pat)
        .and_then(|i| {
            let rest = &line[i + pat.len()..];
            rest.find('"').map(|j| rest[..j].to_string())
        })
        .unwrap_or_default()
}

impl XmlLog {
    /// Returns finished lines (if any) for one raw input line.
    fn feed(&mut self, raw: &str) -> Vec<(Level, String)> {
        let t = raw.trim();
        if !self.in_event {
            if t.starts_with("<log4j:Event") {
                self.in_event = true;
                self.level = attr(t, "level");
                self.thread = attr(t, "thread");
                self.text.clear();
                return vec![];
            }
            return vec![(level_of(raw), raw.to_string())];
        }
        let mut s = raw;
        loop {
            if self.in_cdata {
                if let Some(end) = s.find("]]>") {
                    self.text.push_str(&s[..end]);
                    self.in_cdata = false;
                    s = &s[end + 3..];
                    continue;
                }
                self.text.push_str(s);
                self.text.push('\n');
                return vec![];
            }
            if let Some(start) = s.find("<![CDATA[") {
                if !self.text.is_empty() && !self.text.ends_with('\n') {
                    self.text.push('\n');
                }
                self.in_cdata = true;
                s = &s[start + 9..];
                continue;
            }
            break;
        }
        if t.contains("</log4j:Event>") {
            self.in_event = false;
            let level = match self.level.as_str() {
                "WARN" => Level::Warn,
                "ERROR" | "FATAL" => Level::Error,
                _ => Level::Info,
            };
            let msg = self.text.trim_end().to_string();
            return msg
                .lines()
                .enumerate()
                .map(|(i, l)| {
                    if i == 0 {
                        (level, format!("[{}/{}] {}", self.thread, self.level, l))
                    } else {
                        (level, l.to_string())
                    }
                })
                .collect();
        }
        vec![]
    }
}

fn level_of(line: &str) -> Level {
    if line.contains("/ERROR]") || line.contains("/FATAL]") || line.contains("Exception") {
        Level::Error
    } else if line.contains("/WARN]") {
        Level::Warn
    } else {
        Level::Info
    }
}

/// Spawn a process, stream its output into the session log, and wait for it.
/// With `interactive`, lines sent through the session's `stdin` channel are
/// written to the process (used for the server console).
pub async fn run(spec: LaunchSpec, rep: Reporter, interactive: bool) -> Result<i32> {
    let mut cmd = tokio::process::Command::new(&spec.java);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if interactive { Stdio::piped() } else { Stdio::null() })
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    rep.info(format!("$ {} {}", spec.java.display(), redact(&spec.args).join(" ")));
    let mut child = cmd.spawn().map_err(|e| anyhow!("failed to start {}: {e}", spec.java.display()))?;

    let kill = Arc::new(Notify::new());
    {
        let mut s = rep.session.lock().unwrap();
        s.kill = Some(kill.clone());
    }
    if interactive {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        rep.session.lock().unwrap().stdin = Some(tx);
        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    if stdin.write_all(format!("{line}\n").as_bytes()).await.is_err() {
                        break;
                    }
                    let _ = stdin.flush().await;
                }
            });
        }
    }
    rep.set_state(RunState::Running);
    {
        // set_state(Running) clears nothing for Running, but re-assert handles.
        let mut s = rep.session.lock().unwrap();
        s.kill = Some(kill.clone());
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let r1 = rep.clone();
    let out_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut xml = XmlLog::default();
        while let Ok(Some(line)) = lines.next_line().await {
            for (lvl, text) in xml.feed(&line) {
                r1.log(lvl, text);
            }
        }
    });
    let r2 = rep.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let lvl = if line.contains("WARN") { Level::Warn } else { Level::Error };
            r2.log(lvl, line);
        }
    });

    let status = tokio::select! {
        s = child.wait() => s?,
        _ = kill.notified() => {
            rep.info("Stopping process…");
            let _ = child.start_kill();
            child.wait().await?
        }
    };
    let _ = out_task.await;
    let _ = err_task.await;
    Ok(status.code().unwrap_or(-1))
}

fn redact(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            out.push(format!("<{} entries>", a.split(CP_SEP).count()));
            skip_next = false;
            continue;
        }
        if a == "-cp" || a == "-classpath" {
            skip_next = true;
        }
        out.push(a.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java() {
        // Known value: UUID.nameUUIDFromBytes("OfflinePlayer:Notch")
        assert_eq!(dashed_uuid(&offline_uuid("Notch")), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn xml_log() {
        let mut x = XmlLog::default();
        assert!(x.feed(r#"<log4j:Event logger="a" timestamp="1" level="INFO" thread="main">"#).is_empty());
        assert!(x.feed("  <log4j:Message><![CDATA[Hello world]]></log4j:Message>").is_empty());
        let out = x.feed("</log4j:Event>");
        assert_eq!(out[0].1, "[main/INFO] Hello world");
    }
}
