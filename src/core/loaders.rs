//! Mod loaders: Fabric, Quilt, Forge, NeoForge — version lists and client
//! profile installation. Forge/NeoForge use their installer jars; we run the
//! install-profile processors ourselves (like Prism/HMCL do).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::model::{Library, VersionJson};
use super::net::{self, Dl};
use super::paths::paths;
use super::rules::{self, Env};
use super::session::Reporter;
use super::{java, versions};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    pub const ALL: [Loader; 5] = [Loader::Vanilla, Loader::Fabric, Loader::Quilt, Loader::Forge, Loader::NeoForge];

    pub fn name(self) -> &'static str {
        match self {
            Loader::Vanilla => "Vanilla",
            Loader::Fabric => "Fabric",
            Loader::Quilt => "Quilt",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
        }
    }

    /// Loader name used by Modrinth.
    pub fn modrinth(self) -> Option<&'static str> {
        match self {
            Loader::Vanilla => None,
            Loader::Fabric => Some("fabric"),
            Loader::Quilt => Some("quilt"),
            Loader::Forge => Some("forge"),
            Loader::NeoForge => Some("neoforge"),
        }
    }

    pub fn from_mrpack_key(key: &str) -> Option<Loader> {
        match key {
            "fabric-loader" => Some(Loader::Fabric),
            "quilt-loader" => Some(Loader::Quilt),
            "forge" => Some(Loader::Forge),
            "neoforge" => Some(Loader::NeoForge),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const QUILT_META: &str = "https://meta.quiltmc.org/v3";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_LEGACY_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/forge";

#[derive(Deserialize)]
struct MetaLoaderEntry {
    loader: MetaLoader,
}
#[derive(Deserialize)]
struct MetaLoader {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

fn maven_versions(xml: &str) -> Vec<String> {
    let re = regex::Regex::new(r"<version>([^<]+)</version>").unwrap();
    re.captures_iter(xml).map(|c| c[1].to_string()).collect()
}

/// Compare version strings numerically where possible ("26.2.0.10" > "26.2.0.9").
pub fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

/// The NeoForge version prefix for a Minecraft version:
/// 1.21.1 -> "21.1.", 1.21 -> "21.0.", 26.2 -> "26.2.0.", 26.1.2 -> "26.1.2."
fn neoforge_prefix(mc: &str) -> Option<String> {
    let parts: Vec<&str> = mc.split('.').collect();
    if parts.first() == Some(&"1") {
        let minor = parts.get(1)?;
        let patch = parts.get(2).copied().unwrap_or("0");
        Some(format!("{minor}.{patch}."))
    } else if parts.len() >= 2 && parts[0].chars().all(|c| c.is_ascii_digit()) {
        let patch = parts.get(2).copied().unwrap_or("0");
        Some(format!("{}.{}.{patch}.", parts[0], parts[1]))
    } else {
        None
    }
}

pub async fn list_versions(loader: Loader, mc: &str) -> Result<Vec<LoaderVersion>> {
    let mut out = match loader {
        Loader::Vanilla => vec![],
        Loader::Fabric | Loader::Quilt => {
            let base = if loader == Loader::Fabric { FABRIC_META } else { QUILT_META };
            let list: Vec<MetaLoaderEntry> = net::get_json(&format!("{base}/versions/loader/{mc}")).await?;
            let mut v: Vec<LoaderVersion> = list
                .into_iter()
                .map(|e| {
                    let stable = e.loader.stable.unwrap_or(!e.loader.version.contains("beta") && !e.loader.version.contains("pre"));
                    LoaderVersion { version: e.loader.version, stable }
                })
                .collect();
            v.sort_by(|a, b| cmp_versions(&b.version, &a.version));
            v
        }
        Loader::Forge => {
            let xml = net::get_text(&format!("{FORGE_MAVEN}/maven-metadata.xml")).await?;
            let prefix = format!("{mc}-");
            let promos: HashMap<String, String> = net::get_json::<serde_json::Value>(
                "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
            )
            .await
            .ok()
            .and_then(|v| serde_json::from_value(v.get("promos")?.clone()).ok())
            .unwrap_or_default();
            let recommended = promos.get(&format!("{mc}-recommended")).cloned();
            let mut v: Vec<LoaderVersion> = maven_versions(&xml)
                .into_iter()
                .filter(|v| v.starts_with(&prefix))
                .map(|full| {
                    let short = full.trim_start_matches(&prefix).split('-').next().unwrap_or("").to_string();
                    LoaderVersion { stable: recommended.as_deref() == Some(short.as_str()), version: full }
                })
                .collect();
            v.sort_by(|a, b| cmp_versions(&b.version, &a.version));
            v
        }
        Loader::NeoForge => {
            if mc == "1.20.1" {
                let xml = net::get_text(&format!("{NEOFORGE_LEGACY_MAVEN}/maven-metadata.xml")).await?;
                let mut v: Vec<LoaderVersion> = maven_versions(&xml)
                    .into_iter()
                    .filter(|v| v.starts_with("1.20.1-"))
                    .map(|v| LoaderVersion { stable: true, version: v })
                    .collect();
                v.sort_by(|a, b| cmp_versions(&b.version, &a.version));
                v
            } else {
                let prefix = neoforge_prefix(mc).ok_or_else(|| anyhow!("NeoForge doesn't support {mc}"))?;
                let xml = net::get_text(&format!("{NEOFORGE_MAVEN}/maven-metadata.xml")).await?;
                let mut v: Vec<LoaderVersion> = maven_versions(&xml)
                    .into_iter()
                    .filter(|v| v.starts_with(&prefix) && !v.contains('+'))
                    .map(|v| LoaderVersion { stable: !v.contains("beta") && !v.contains("alpha"), version: v })
                    .collect();
                v.sort_by(|a, b| cmp_versions(&b.version, &a.version));
                v
            }
        }
    };
    // Put a stable version first if there is one (it becomes the default).
    if let Some(i) = out.iter().position(|v| v.stable) {
        let item = out.remove(i);
        out.insert(0, item);
    }
    Ok(out)
}

/// Install the loader profile for a client and return the version id to launch.
pub async fn install_client(loader: Loader, mc: &str, lv: &str, rep: &Reporter) -> Result<String> {
    match loader {
        Loader::Vanilla => Ok(mc.to_string()),
        Loader::Fabric | Loader::Quilt => {
            let base = if loader == Loader::Fabric { FABRIC_META } else { QUILT_META };
            rep.stage(format!("Installing {} {lv}", loader.name()));
            let url = format!("{base}/versions/loader/{mc}/{lv}/profile/json");
            let bytes = net::get_bytes(&url).await?;
            let json: VersionJson = serde_json::from_slice(&bytes).context("loader profile")?;
            versions::save_json(&json)?;
            Ok(json.id)
        }
        Loader::Forge | Loader::NeoForge => {
            let installer = download_installer(loader, mc, lv, rep).await?;
            run_installer_client(&installer, mc, rep).await
        }
    }
}

fn installer_url(loader: Loader, mc: &str, lv: &str) -> String {
    match loader {
        Loader::Forge => format!("{FORGE_MAVEN}/{lv}/forge-{lv}-installer.jar"),
        Loader::NeoForge if mc == "1.20.1" => format!("{NEOFORGE_LEGACY_MAVEN}/{lv}/forge-{lv}-installer.jar"),
        _ => format!("{NEOFORGE_MAVEN}/{lv}/neoforge-{lv}-installer.jar"),
    }
}

pub async fn download_installer(loader: Loader, mc: &str, lv: &str, rep: &Reporter) -> Result<PathBuf> {
    let url = installer_url(loader, mc, lv);
    let file = url.rsplit('/').next().unwrap().to_string();
    let path = paths().cache().join("installers").join(file);
    if !path.exists() {
        rep.stage(format!("Downloading {} installer", loader.name()));
        net::download_one(&Dl::new(url, &path)).await?;
    }
    Ok(path)
}

fn zip_read(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>> {
    let name = name.trim_start_matches('/');
    let mut f = zip.by_name(name).with_context(|| format!("{name} missing from installer"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

#[derive(Deserialize)]
struct InstallProfile {
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    data: HashMap<String, SidedData>,
    #[serde(default)]
    processors: Vec<Processor>,
    #[serde(default)]
    libraries: Vec<Library>,
    // Legacy (<= 1.12.2) installers:
    #[serde(default)]
    install: Option<LegacyInstall>,
    #[serde(rename = "versionInfo", default)]
    version_info: Option<VersionJson>,
}

#[derive(Deserialize)]
struct SidedData {
    client: String,
    server: String,
}

#[derive(Deserialize)]
struct Processor {
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    outputs: HashMap<String, String>,
    #[serde(default)]
    sides: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct LegacyInstall {
    path: String,
    #[serde(rename = "filePath")]
    file_path: String,
    minecraft: String,
}

fn lib_path(coord: &str) -> PathBuf {
    paths().libraries().join(rules::maven_path(coord).unwrap_or_default())
}

/// Install a Forge/NeoForge client from its installer. Returns the version id.
async fn run_installer_client(installer: &Path, mc: &str, rep: &Reporter) -> Result<String> {
    rep.stage("Reading installer");
    let file = std::fs::File::open(installer)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let profile: InstallProfile = serde_json::from_slice(&zip_read(&mut zip, "install_profile.json")?)?;

    // ---- Legacy Forge (1.12.2 and older) ----
    if let (Some(install), Some(mut vi)) = (profile.install, profile.version_info) {
        let dst = lib_path(&install.path);
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&dst, zip_read(&mut zip, &install.file_path)?)?;
        if vi.inherits_from.is_none() {
            vi.inherits_from = Some(install.minecraft.clone());
        }
        // Legacy profiles often omit jar; use the vanilla one.
        if vi.jar.is_none() {
            vi.jar = Some(install.minecraft.clone());
        }
        // Don't let legacy Forge override vanilla's asset index or downloads.
        vi.asset_index = None;
        vi.assets = None;
        vi.downloads = None;
        versions::save_json(&vi)?;
        return Ok(vi.id);
    }

    // ---- Modern installer (1.13+) with processors ----
    let version_json: VersionJson = serde_json::from_slice(&zip_read(
        &mut zip,
        profile.json.as_deref().unwrap_or("/version.json"),
    )?)?;
    let env = Env::host();

    // Vanilla first: processors need the client jar.
    let vanilla = versions::resolve(mc).await?;
    versions::install_client_files(&vanilla, &env, rep).await?;

    // Bundled maven artifacts inside the installer.
    rep.stage("Extracting installer libraries");
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.to_string();
        if let Some(rel) = name.strip_prefix("maven/") {
            if entry.is_dir() || rel.is_empty() {
                continue;
            }
            let dst = paths().libraries().join(rel);
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::File::create(&dst)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }

    // Libraries needed by processors + the profile itself.
    let mut dls = Vec::new();
    for lib in profile.libraries.iter().chain(version_json.libraries.iter()) {
        if !rules::library_wanted(lib, &env) {
            continue;
        }
        if let Some(a) = rules::main_artifact(lib) {
            if !a.urls.is_empty() {
                dls.push(Dl { urls: a.urls, path: paths().libraries().join(&a.rel), sha1: a.sha1, size: a.size });
            }
        }
    }
    net::download_all(dls, rep, "Downloading loader libraries", true).await?;

    // Processors.
    let (java, _) = java::for_mc_version(mc, None, rep).await?;
    let work = paths().cache().join("installer-work").join(&version_json.id);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;

    let mut data: HashMap<String, String> = HashMap::new();
    for (k, v) in &profile.data {
        data.insert(k.clone(), resolve_data(&v.client, &mut zip, &work)?);
    }
    data.insert("SIDE".into(), "client".into());
    data.insert("MINECRAFT_JAR".into(), paths().version_jar(&vanilla.jar_id).to_string_lossy().into());
    data.insert("MINECRAFT_VERSION".into(), mc.into());
    data.insert("ROOT".into(), paths().root.to_string_lossy().into());
    data.insert("INSTALLER".into(), installer.to_string_lossy().into());
    data.insert("LIBRARY_DIR".into(), paths().libraries().to_string_lossy().into());

    let procs: Vec<&Processor> = profile
        .processors
        .iter()
        .filter(|p| p.sides.as_ref().map(|s| s.iter().any(|s| s == "client")).unwrap_or(true))
        .collect();
    for (i, p) in procs.iter().enumerate() {
        rep.stage(format!("Running installer step {}/{}", i + 1, procs.len()));
        run_processor(p, &data, &java, &work, rep).await?;
    }
    let _ = std::fs::remove_dir_all(&work);

    versions::save_json(&version_json)?;
    Ok(version_json.id)
}

fn resolve_data(value: &str, zip: &mut zip::ZipArchive<std::fs::File>, work: &Path) -> Result<String> {
    if let Some(coord) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return Ok(lib_path(coord).to_string_lossy().into());
    }
    if let Some(lit) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return Ok(lit.to_string());
    }
    if value.starts_with('/') {
        let dst = work.join(value.trim_start_matches('/'));
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&dst, zip_read(zip, value)?)?;
        return Ok(dst.to_string_lossy().into());
    }
    Ok(value.to_string())
}

fn sub_arg(arg: &str, data: &HashMap<String, String>) -> String {
    if let Some(key) = arg.strip_prefix('{').and_then(|a| a.strip_suffix('}')) {
        return data.get(key).cloned().unwrap_or_else(|| arg.to_string());
    }
    if let Some(coord) = arg.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
        return lib_path(coord).to_string_lossy().into();
    }
    // Embedded {KEY} tokens
    let mut out = arg.to_string();
    for (k, v) in data {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

fn jar_main_class(jar: &Path) -> Result<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).with_context(|| format!("missing {}", jar.display()))?)?;
    let mut s = String::new();
    zip.by_name("META-INF/MANIFEST.MF")?.read_to_string(&mut s)?;
    // Manifest lines can wrap with a leading space.
    let unwrapped = s.replace("\r\n ", "").replace("\n ", "");
    unwrapped
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:").map(|v| v.trim().to_string()))
        .ok_or_else(|| anyhow!("{} has no Main-Class", jar.display()))
}

async fn run_processor(
    p: &Processor,
    data: &HashMap<String, String>,
    java: &Path,
    work: &Path,
    rep: &Reporter,
) -> Result<()> {
    // Skip if all declared outputs already exist with the right hash.
    if !p.outputs.is_empty() {
        let all_ok = p.outputs.iter().all(|(k, v)| {
            let path = sub_arg(k, data);
            let expected = sub_arg(v, data).trim_matches('\'').to_string();
            net::sha1_file(Path::new(&path)).map(|h| h.eq_ignore_ascii_case(&expected)).unwrap_or(false)
        });
        if all_ok {
            return Ok(());
        }
    }
    let jar = lib_path(&p.jar);
    let main = jar_main_class(&jar)?;
    let mut cp = vec![jar.to_string_lossy().to_string()];
    cp.extend(p.classpath.iter().map(|c| lib_path(c).to_string_lossy().to_string()));
    let mut args = vec!["-cp".to_string(), cp.join(super::launch::CP_SEP), main];
    args.extend(p.args.iter().map(|a| sub_arg(a, data)));

    let spec = super::launch::LaunchSpec { java: java.to_path_buf(), args, cwd: work.to_path_buf() };
    let code = super::launch::run(spec, rep.clone(), false).await?;
    rep.set_state(super::session::RunState::Working);
    if code != 0 {
        bail!("installer step {} failed (exit code {code})", p.jar);
    }
    Ok(())
}

// ------------------------------------------------------------------ servers

/// How to start an installed server.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ServerLaunch {
    /// Arguments after `java` and the memory flags (e.g. `-jar server.jar`).
    pub args: Vec<String>,
}

/// Install a server of any loader into `dir`.
pub async fn install_server(loader: Loader, mc: &str, lv: Option<&str>, dir: &Path, rep: &Reporter) -> Result<ServerLaunch> {
    std::fs::create_dir_all(dir)?;
    let vanilla = versions::server_jar_download(mc).await?;
    net::download_all(vec![vanilla.clone()], rep, "Downloading server", true).await?;
    let server_jar = dir.join("server.jar");
    std::fs::copy(&vanilla.path, &server_jar)?;

    match loader {
        Loader::Vanilla => Ok(ServerLaunch { args: vec!["-jar".into(), "server.jar".into(), "nogui".into()] }),
        Loader::Fabric | Loader::Quilt => {
            let lv = lv.ok_or_else(|| anyhow!("no loader version"))?;
            let base = if loader == Loader::Fabric { FABRIC_META } else { QUILT_META };
            rep.stage(format!("Installing {} {lv} server", loader.name()));
            let profile: VersionJson = net::get_json(&format!("{base}/versions/loader/{mc}/{lv}/server/json")).await?;
            let env = Env::host();
            let mut cp = Vec::new();
            let mut dls = Vec::new();
            for lib in &profile.libraries {
                if !rules::library_wanted(lib, &env) {
                    continue;
                }
                if let Some(a) = rules::main_artifact(lib) {
                    let path = paths().libraries().join(&a.rel);
                    cp.push(path.to_string_lossy().to_string());
                    dls.push(Dl { urls: a.urls, path, sha1: a.sha1, size: a.size });
                }
            }
            net::download_all(dls, rep, "Downloading loader libraries", true).await?;
            let main = if loader == Loader::Fabric {
                std::fs::write(dir.join("fabric-server-launcher.properties"), "serverJar=server.jar\n")?;
                "net.fabricmc.loader.impl.launch.server.FabricServerLauncher"
            } else {
                std::fs::write(dir.join("quilt-server-launcher.properties"), "serverJar=server.jar\n")?;
                "org.quiltmc.loader.impl.launch.server.QuiltServerLauncher"
            };
            Ok(ServerLaunch {
                args: vec!["-cp".into(), cp.join(super::launch::CP_SEP), main.into(), "nogui".into()],
            })
        }
        Loader::Forge | Loader::NeoForge => {
            let lv = lv.ok_or_else(|| anyhow!("no loader version"))?;
            let installer = download_installer(loader, mc, lv, rep).await?;
            // Old installers try to fetch the vanilla jar from dead URLs; pre-place it.
            std::fs::copy(&vanilla.path, dir.join(format!("minecraft_server.{mc}.jar")))?;
            let (java, _) = java::for_mc_version(mc, None, rep).await?;
            rep.stage(format!("Running {} server installer", loader.name()));
            let spec = super::launch::LaunchSpec {
                java,
                args: vec!["-jar".into(), installer.to_string_lossy().into(), "--installServer".into(), dir.to_string_lossy().into()],
                cwd: dir.to_path_buf(),
            };
            let code = super::launch::run(spec, rep.clone(), false).await?;
            rep.set_state(super::session::RunState::Working);
            if code != 0 {
                bail!("{} server installer failed (exit code {code})", loader.name());
            }
            for leftover in ["installer.log"] {
                let _ = std::fs::remove_file(dir.join(leftover));
            }
            detect_forge_server_launch(dir)
        }
    }
}

/// Work out how to start a Forge/NeoForge server after its installer ran.
fn detect_forge_server_launch(dir: &Path) -> Result<ServerLaunch> {
    // Modern installers write run.sh with the exact java arguments.
    if let Ok(script) = std::fs::read_to_string(dir.join("run.sh")) {
        for line in script.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("java ") {
                let args: Vec<String> = rest
                    .split_whitespace()
                    .filter(|a| *a != "@user_jvm_args.txt" && *a != "\"$@\"" && *a != "$@")
                    .map(|s| s.to_string())
                    .collect();
                if !args.is_empty() {
                    let mut args = args;
                    args.push("nogui".into());
                    return Ok(ServerLaunch { args });
                }
            }
        }
    }
    // Older installers produce forge-<ver>.jar / forge-<ver>-universal.jar.
    let mut jars: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            (n.starts_with("forge-") || n.starts_with("neoforge-"))
                && n.ends_with(".jar")
                && !n.contains("installer")
        })
        .collect();
    jars.sort();
    if let Some(j) = jars.into_iter().next() {
        return Ok(ServerLaunch { args: vec!["-jar".into(), j, "nogui".into()] });
    }
    bail!("couldn't find how to start the server after installing")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neo_prefix() {
        assert_eq!(neoforge_prefix("1.21.1").unwrap(), "21.1.");
        assert_eq!(neoforge_prefix("1.21").unwrap(), "21.0.");
        assert_eq!(neoforge_prefix("26.2").unwrap(), "26.2.0.");
        assert_eq!(neoforge_prefix("26.1.2").unwrap(), "26.1.2.");
    }
    #[test]
    fn version_order() {
        assert_eq!(cmp_versions("26.2.0.10", "26.2.0.9"), std::cmp::Ordering::Greater);
    }
}
