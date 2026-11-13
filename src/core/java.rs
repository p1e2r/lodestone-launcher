//! Downloads Mojang's own Java runtimes, so the user never has to install
//! Java. Picks the runtime the version JSON asks for (`javaVersion`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use super::model::VersionJson;
use super::net::{self, Dl};
use super::paths::paths;
use super::rules::Env;
use super::session::Reporter;

const RUNTIMES_URL: &str = "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Deserialize, Clone)]
struct RuntimeEntry {
    manifest: RuntimeManifestRef,
    version: RuntimeVersion,
}
#[derive(Deserialize, Clone)]
struct RuntimeManifestRef {
    url: String,
    #[serde(default)]
    sha1: Option<String>,
}
#[derive(Deserialize, Clone)]
struct RuntimeVersion {
    name: String,
}
type AllRuntimes = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Deserialize)]
struct RuntimeFiles {
    files: HashMap<String, RuntimeFile>,
}
#[derive(Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    downloads: Option<HashMap<String, RuntimeDownload>>,
    #[serde(default)]
    target: Option<String>,
}
#[derive(Deserialize)]
struct RuntimeDownload {
    url: String,
    sha1: String,
    size: u64,
}

/// What Java a version needs and which platform build to use.
#[derive(Clone, Debug)]
pub struct JavaPlan {
    pub component: String,
    pub major: u32,
    pub platform: String,
    /// Architecture of the chosen Java build ("x86_64" when using Rosetta).
    pub arch: String,
}

fn platform_key(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("osx", "aarch64") => "mac-os-arm64",
        ("osx", _) => "mac-os",
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("windows", _) => "windows-x64",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

/// Decide which Java to use. Old versions (LWJGL 2 natives) have no Apple
/// Silicon natives, so on arm64 Macs they run under Rosetta with x86 Java.
pub async fn plan(json: &VersionJson, env: &Env) -> Result<JavaPlan> {
    let component = json
        .java_version
        .as_ref()
        .and_then(|j| j.component.clone())
        .unwrap_or_else(|| "jre-legacy".to_string());
    let major = json.java_version.as_ref().map(|j| j.major_version).unwrap_or(8);
    let all: AllRuntimes = net::cached_json(RUNTIMES_URL, "java_runtimes.json").await?;

    let mut arch = env.arch.clone();
    if env.os == "osx" && env.is_arm() && super::versions::legacy_natives(json) {
        arch = "x86_64".into();
    }
    let mut platform = platform_key(env.os, &arch).to_string();
    let available = |p: &str| {
        all.get(p)
            .and_then(|m| m.get(&component))
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    };
    if !available(&platform) {
        // Fall back to emulated x64 builds on ARM Macs / Windows.
        let fallback = match platform.as_str() {
            "mac-os-arm64" => Some("mac-os"),
            "windows-arm64" => Some("windows-x64"),
            _ => None,
        };
        if let Some(fb) = fallback.filter(|fb| available(fb)) {
            platform = fb.to_string();
            arch = "x86_64".into();
        }
    }
    Ok(JavaPlan { component, major, platform, arch })
}

fn java_exe(dir: &Path, platform: &str) -> PathBuf {
    if platform.starts_with("mac-os") {
        dir.join("jre.bundle/Contents/Home/bin/java")
    } else if platform.starts_with("windows") {
        dir.join("bin").join("javaw.exe")
    } else {
        dir.join("bin").join("java")
    }
}

/// Make sure the planned runtime is installed and return the `java` path.
/// `override_path` (from settings) wins if set.
pub async fn ensure(plan: &JavaPlan, override_path: Option<&Path>, rep: &Reporter) -> Result<PathBuf> {
    if let Some(p) = override_path {
        return Ok(p.to_path_buf());
    }
    let all: AllRuntimes = net::cached_json(RUNTIMES_URL, "java_runtimes.json").await?;
    let Some(entry) = all
        .get(&plan.platform)
        .and_then(|m| m.get(&plan.component))
        .and_then(|v| v.first())
        .cloned()
    else {
        rep.warn(format!(
            "Mojang has no Java {} build for {}; using `java` from PATH",
            plan.major, plan.platform
        ));
        return Ok(PathBuf::from("java"));
    };

    let dir = paths().runtime().join(&plan.component).join(&plan.platform);
    let marker = dir.join(".lodestone-version");
    let exe = java_exe(&dir, &plan.platform);
    if std::fs::read_to_string(&marker).ok().as_deref() == Some(entry.version.name.as_str()) && exe.exists() {
        return Ok(exe);
    }

    rep.stage(format!("Installing Java {}", entry.version.name));
    let files: RuntimeFiles = net::get_json(&entry.manifest.url).await.context("Java runtime manifest")?;
    let _ = entry.manifest.sha1;
    let mut dls = Vec::new();
    let mut links = Vec::new();
    let mut executables = Vec::new();
    for (rel, f) in &files.files {
        let path = dir.join(rel);
        match f.kind.as_str() {
            "directory" => std::fs::create_dir_all(&path)?,
            "file" => {
                let raw = f
                    .downloads
                    .as_ref()
                    .and_then(|d| d.get("raw"))
                    .ok_or_else(|| anyhow!("runtime file {rel} has no download"))?;
                dls.push(Dl::new(&raw.url, &path).sha1(Some(raw.sha1.clone())).size(Some(raw.size)));
                if f.executable {
                    executables.push(path);
                }
            }
            "link" => {
                if let Some(t) = &f.target {
                    links.push((path, t.clone()));
                }
            }
            _ => {}
        }
    }
    net::download_all(dls, rep, "Downloading Java", true).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in &executables {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
        }
        for (path, target) in &links {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::remove_file(path);
            let _ = std::os::unix::fs::symlink(target, path);
        }
    }
    #[cfg(not(unix))]
    let _ = (&executables, &links);

    std::fs::write(&marker, &entry.version.name)?;
    Ok(exe)
}

/// Java for running installers/servers for a given vanilla version.
pub async fn for_mc_version(mc: &str, override_path: Option<&Path>, rep: &Reporter) -> Result<(PathBuf, JavaPlan)> {
    let json = super::versions::ensure_json(mc).await?;
    let env = Env::host();
    let mut plan = plan(&json, &env).await?;
    // Servers have no natives, so they can always use native-arch Java.
    if env.os == "osx" && env.is_arm() && plan.platform == "mac-os" {
        let native = JavaPlan { platform: "mac-os-arm64".into(), arch: "aarch64".into(), ..plan.clone() };
        let all: AllRuntimes = net::cached_json(RUNTIMES_URL, "java_runtimes.json").await?;
        if all.get("mac-os-arm64").and_then(|m| m.get(&plan.component)).map(|v| !v.is_empty()).unwrap_or(false) {
            plan = native;
        }
    }
    let java = ensure(&plan, override_path, rep).await?;
    Ok((java, plan))
}
