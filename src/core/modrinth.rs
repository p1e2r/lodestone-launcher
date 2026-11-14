//! Modrinth (Labrinth v2 API): search, install with dependencies, and
//! `.mrpack` modpacks.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::instance::{Instance, Kind};
use super::loaders::Loader;
use super::net::{self, Dl, HTTP};
use super::paths::paths;
use super::session::Reporter;

const DEFAULT_API: &str = "https://api.modrinth.com/v2";

/// Modrinth API base. `LODESTONE_MODRINTH_API` overrides it (for testing).
fn api() -> &'static str {
    static API: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    API.get_or_init(|| std::env::var("LODESTONE_MODRINTH_API").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_API.to_string()))
}
pub const FABRIC_API_ID: &str = "P7dR8mSH";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    Mod,
    Modpack,
    ResourcePack,
    Shader,
}

impl ContentType {
    pub const ALL: [ContentType; 4] = [ContentType::Mod, ContentType::Modpack, ContentType::ResourcePack, ContentType::Shader];
    pub fn api(self) -> &'static str {
        match self {
            ContentType::Mod => "mod",
            ContentType::Modpack => "modpack",
            ContentType::ResourcePack => "resourcepack",
            ContentType::Shader => "shader",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ContentType::Mod => "Mods",
            ContentType::Modpack => "Modpacks",
            ContentType::ResourcePack => "Resource Packs",
            ContentType::Shader => "Shaders",
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            ContentType::Mod | ContentType::Modpack => "mods",
            ContentType::ResourcePack => "resourcepacks",
            ContentType::Shader => "shaderpacks",
        }
    }
    fn from_api(s: &str) -> ContentType {
        match s {
            "modpack" => ContentType::Modpack,
            "resourcepack" => ContentType::ResourcePack,
            "shader" => ContentType::Shader,
            _ => ContentType::Mod,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub categories: Vec<String>,
    pub project_type: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct MVersion {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub name: String,
    pub version_number: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    pub files: Vec<MFile>,
    #[serde(default)]
    pub dependencies: Vec<MDep>,
    #[serde(default)]
    pub date_published: String,
    /// "release", "beta" or "alpha".
    #[serde(default)]
    pub version_type: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct MFile {
    pub hashes: HashMap<String, String>,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct MDep {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Deserialize)]
struct Project {
    id: String,
    title: String,
    project_type: String,
}

/// Loader names to filter Modrinth by. Quilt also runs Fabric mods.
fn loader_filter(loader: Loader) -> Vec<&'static str> {
    match loader {
        Loader::Quilt => vec!["quilt", "fabric"],
        l => l.modrinth().into_iter().collect(),
    }
}

pub async fn search(
    query: &str,
    ct: ContentType,
    target: Option<&Instance>,
    offset: u32,
) -> Result<SearchResponse> {
    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", ct.api())]];
    if let Some(inst) = target {
        if ct != ContentType::Modpack {
            facets.push(vec![format!("versions:{}", inst.mc_version)]);
        }
        if ct == ContentType::Mod {
            let l = loader_filter(inst.loader);
            if !l.is_empty() {
                facets.push(l.iter().map(|l| format!("categories:{l}")).collect());
            }
            if inst.kind == Kind::Server {
                facets.push(vec!["server_side:required".into(), "server_side:optional".into()]);
            } else {
                facets.push(vec!["client_side:required".into(), "client_side:optional".into()]);
            }
        }
    }
    let facets = serde_json::to_string(&facets)?;
    let index = if query.trim().is_empty() { "downloads" } else { "relevance" };
    let resp = HTTP
        .get(format!("{}/search", api()))
        .query(&[
            ("query", query),
            ("facets", facets.as_str()),
            ("index", index),
            ("limit", "20"),
            ("offset", &offset.to_string()),
        ])
        .send()
        .await
        .context("couldn't reach Modrinth")?
        .error_for_status()?;
    Ok(resp.json().await?)
}

pub async fn project_versions(id: &str, loaders: &[&str], game_version: Option<&str>) -> Result<Vec<MVersion>> {
    let mut q: Vec<(&str, String)> = Vec::new();
    if !loaders.is_empty() {
        q.push(("loaders", serde_json::to_string(loaders)?));
    }
    if let Some(g) = game_version {
        q.push(("game_versions", serde_json::to_string(&[g])?));
    }
    let resp = HTTP.get(format!("{}/project/{id}/version", api())).query(&q).send().await?.error_for_status()?;
    Ok(resp.json().await?)
}

pub async fn get_version(id: &str) -> Result<MVersion> {
    net::get_json(&format!("{}/version/{id}", api())).await
}

/// Whether a version runs on the instance's Minecraft version (and, for mods, its loader).
pub fn is_compatible(inst: &Instance, v: &MVersion, ct: ContentType) -> bool {
    if !v.game_versions.iter().any(|g| g == &inst.mc_version) {
        return false;
    }
    if ct == ContentType::Mod {
        let l = loader_filter(inst.loader);
        return l.is_empty() || v.loaders.iter().any(|x| l.contains(&x.as_str()));
    }
    true
}

/// Versions of a project that work with an instance, newest first.
pub async fn compatible_versions(inst: &Instance, project_id: &str) -> Result<Vec<MVersion>> {
    let project: Project = net::get_json(&format!("{}/project/{project_id}", api())).await?;
    let ct = ContentType::from_api(&project.project_type);
    let loaders = if ct == ContentType::Mod { loader_filter(inst.loader) } else { vec![] };
    let list = project_versions(&project.id, &loaders, Some(&inst.mc_version)).await?;
    Ok(list.into_iter().filter(|v| is_compatible(inst, v, ct)).collect())
}

/// Every version of a modpack, newest first.
pub async fn modpack_versions(project_id: &str) -> Result<Vec<MVersion>> {
    project_versions(project_id, &[], None).await
}

async fn latest_compatible(inst: &Instance, project: &Project, ct: ContentType) -> Result<MVersion> {
    let loaders = if ct == ContentType::Mod { loader_filter(inst.loader) } else { vec![] };
    let list = project_versions(&project.id, &loaders, Some(&inst.mc_version)).await?;
    list.into_iter().find(|v| is_compatible(inst, v, ct)).ok_or_else(|| {
        anyhow!("{} has no version for Minecraft {} {}", project.title, inst.mc_version, if ct == ContentType::Mod { inst.loader.name() } else { "" })
    })
}

/// What Lodestone installed from Modrinth into an instance.
#[derive(Serialize, Deserialize, Default)]
pub struct InstalledIndex {
    pub projects: HashMap<String, InstalledProject>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InstalledProject {
    pub title: String,
    pub file: String,
    pub folder: String,
    pub version: String,
}

impl InstalledIndex {
    fn path(inst: &Instance) -> PathBuf {
        inst.dir().join("modrinth.json")
    }
    pub fn load(inst: &Instance) -> InstalledIndex {
        std::fs::read(Self::path(inst)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    fn save(&self, inst: &Instance) -> Result<()> {
        std::fs::write(Self::path(inst), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
    /// Installed and the file is still there.
    pub fn has(&self, inst: &Instance, project: &str) -> bool {
        self.projects.get(project).map(|p| {
            let path = inst.content_dir(&p.folder).join(&p.file);
            path.exists() || PathBuf::from(format!("{}.disabled", path.to_string_lossy())).exists()
        }).unwrap_or(false)
    }
}

fn primary(v: &MVersion) -> Option<&MFile> {
    v.files.iter().find(|f| f.primary).or(v.files.first())
}

/// Install a project (and its required dependencies) into an instance.
/// `version_id` picks a specific version; it must be compatible with the
/// instance. `None` installs the newest compatible version.
pub async fn install_project(inst: &Instance, project_id: &str, version_id: Option<String>, rep: &Reporter) -> Result<Vec<String>> {
    let mut index = InstalledIndex::load(inst);
    let mut queue: VecDeque<(String, Option<String>)> = VecDeque::from([(project_id.to_string(), version_id)]);
    let mut done: HashSet<String> = HashSet::new();
    let mut installed = Vec::new();

    while let Some((pid, version_id)) = queue.pop_front() {
        if !done.insert(pid.clone()) {
            continue;
        }
        if pid != project_id && index.has(inst, &pid) {
            continue;
        }
        let project: Project = net::get_json(&format!("{}/project/{pid}", api())).await?;
        let ct = ContentType::from_api(&project.project_type);
        if ct == ContentType::Modpack {
            bail!("{} is a modpack — install it from the Modpacks tab", project.title);
        }
        let version = match version_id {
            Some(vid) => {
                let v = get_version(&vid).await?;
                if is_compatible(inst, &v, ct) {
                    v
                } else if pid == project_id {
                    bail!("{} {} doesn't work with Minecraft {} {}", project.title, v.version_number, inst.mc_version, inst.loader.name());
                } else {
                    // A dependency pinned to an incompatible version: use the newest one that fits.
                    latest_compatible(inst, &project, ct).await?
                }
            }
            None => latest_compatible(inst, &project, ct).await?,
        };
        let file = primary(&version).ok_or_else(|| anyhow!("{} has no files", project.title))?;
        let folder = ct.folder();
        let dest = inst.content_dir(folder).join(&file.filename);
        rep.stage(format!("Installing {} {}", project.title, version.version_number));
        // Replace an older file of the same project.
        if let Some(old) = index.projects.get(&project.id) {
            if old.file != file.filename {
                let _ = std::fs::remove_file(inst.content_dir(&old.folder).join(&old.file));
            }
        }
        net::download_one(&Dl::new(&file.url, &dest).sha1(file.hashes.get("sha1").cloned()).size(Some(file.size))).await?;
        index.projects.insert(
            project.id.clone(),
            InstalledProject { title: project.title.clone(), file: file.filename.clone(), folder: folder.into(), version: version.version_number.clone() },
        );
        index.save(inst)?;
        installed.push(project.title.clone());

        for dep in version.dependencies.iter().filter(|d| d.dependency_type == "required") {
            let dep_pid = match (&dep.project_id, &dep.version_id) {
                (Some(p), _) => p.clone(),
                (None, Some(v)) => get_version(v).await?.project_id,
                _ => continue,
            };
            queue.push_back((dep_pid, dep.version_id.clone()));
        }
    }
    Ok(installed)
}

pub fn remove_project(inst: &Instance, project_id: &str) -> Result<()> {
    let mut index = InstalledIndex::load(inst);
    if let Some(p) = index.projects.remove(project_id) {
        let _ = std::fs::remove_file(inst.content_dir(&p.folder).join(&p.file));
    }
    index.save(inst)
}

// ------------------------------------------------------------------ identify existing files

/// Identify files already in an instance (from a modpack, dropped in by
/// hand, …) by their SHA-1 on Modrinth, and record them as installed so
/// Discover shows them as "Installed". Returns how many were identified.
pub async fn sync_installed(inst: &Instance) -> Result<usize> {
    let mut index = InstalledIndex::load(inst);
    let known: HashSet<(String, String)> = index.projects.values().map(|p| (p.folder.clone(), p.file.clone())).collect();

    // Hash files we don't know yet.
    let mut pending: Vec<(String, String, PathBuf)> = Vec::new(); // (folder, file name, path)
    for folder in ["mods", "resourcepacks", "shaderpacks"] {
        for f in inst.list_content(folder) {
            if !known.contains(&(folder.to_string(), f.display.clone())) {
                pending.push((folder.to_string(), f.display.clone(), f.path.clone()));
            }
        }
    }
    if pending.is_empty() {
        return Ok(0);
    }
    let hashed: Vec<(String, String, String)> = tokio::task::spawn_blocking(move || {
        pending
            .into_iter()
            .filter_map(|(folder, name, path)| net::sha1_file(&path).ok().map(|h| (folder, name, h)))
            .collect()
    })
    .await?;

    // Ask Modrinth which versions those hashes belong to (one request).
    let hashes: Vec<&str> = hashed.iter().map(|(_, _, h)| h.as_str()).collect();
    let found = versions_by_hash(&hashes).await?;
    if found.is_empty() {
        return Ok(0);
    }

    // Project titles (one request).
    let ids: Vec<String> = found.values().map(|v| v.project_id.clone()).collect::<HashSet<_>>().into_iter().collect();
    let projects: Vec<Project> = HTTP
        .get(format!("{}/projects", api()))
        .query(&[("ids", serde_json::to_string(&ids)?)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let titles: HashMap<String, String> = projects.into_iter().map(|p| (p.id, p.title)).collect();

    let mut added = 0;
    for (folder, name, hash) in hashed {
        let Some(v) = found.get(&hash) else { continue };
        index.projects.insert(
            v.project_id.clone(),
            InstalledProject {
                title: titles.get(&v.project_id).cloned().unwrap_or_else(|| name.clone()),
                file: name,
                folder,
                version: v.version_number.clone(),
            },
        );
        added += 1;
    }
    index.save(inst)?;
    Ok(added)
}

/// Modrinth versions for SHA-1 hashes (one request).
async fn versions_by_hash(hashes: &[&str]) -> Result<HashMap<String, MVersion>> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    let resp = HTTP
        .post(format!("{}/version_files", api()))
        .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }))
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json().await?)
}

// ------------------------------------------------------------------ .mrpack

#[derive(Deserialize)]
struct MrIndex {
    name: String,
    #[serde(rename = "versionId", default)]
    version_id: String,
    files: Vec<MrFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
struct MrFile {
    path: String,
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
    downloads: Vec<String>,
    #[serde(rename = "fileSize", default)]
    file_size: u64,
}

fn safe_rel(p: &str) -> Option<PathBuf> {
    let path = Path::new(p);
    if path.components().all(|c| matches!(c, Component::Normal(_))) {
        Some(path.to_path_buf())
    } else {
        None
    }
}

async fn download_pack(v: &MVersion) -> Result<PathBuf> {
    let f = primary(v).ok_or_else(|| anyhow!("modpack has no files"))?;
    let path = paths().cache().join("modpacks").join(&f.filename);
    net::download_one(&Dl::new(&f.url, &path).sha1(f.hashes.get("sha1").cloned())).await?;
    Ok(path)
}

/// Install a Modrinth modpack as a new instance. `version_id` picks a version;
/// `None` uses the newest.
pub async fn install_modpack(project_id: &str, version_id: Option<String>, kind: Kind, rep: &Reporter) -> Result<Instance> {
    rep.stage("Fetching modpack");
    let v = match version_id {
        Some(id) => get_version(&id).await?,
        None => modpack_versions(project_id).await?.into_iter().next().ok_or_else(|| anyhow!("modpack has no versions"))?,
    };
    let path = download_pack(&v).await?;
    let mut inst = install_mrpack(&path, kind, Some(project_id.to_string()), rep).await?;
    inst.modpack_version = Some(v.id.clone());
    inst.modpack_version_name = Some(v.version_number.clone());
    inst.save()?;
    // Use the modpack's own icon.
    if let Ok(p) = net::get_json::<serde_json::Value>(&format!("{}/project/{project_id}", api())).await {
        if let Some(url) = p.get("icon_url").and_then(|u| u.as_str()) {
            if let Ok(icon) = cached_icon(url).await {
                let _ = inst.set_icon_from(&icon);
            }
        }
    }
    Ok(inst)
}

fn read_index(path: &Path) -> Result<(zip::ZipArchive<std::fs::File>, MrIndex)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let index: MrIndex = {
        let mut s = String::new();
        zip.by_name("modrinth.index.json").context("not a Modrinth modpack (no modrinth.index.json)")?.read_to_string(&mut s)?;
        serde_json::from_str(&s)?
    };
    Ok((zip, index))
}

/// Minecraft version, loader and loader version a pack needs.
fn pack_target(index: &MrIndex) -> Result<(String, Loader, Option<String>)> {
    let mc = index.dependencies.get("minecraft").cloned().ok_or_else(|| anyhow!("modpack doesn't say which Minecraft version it needs"))?;
    let (loader, lv) = index
        .dependencies
        .iter()
        .find_map(|(k, v)| Loader::from_mrpack_key(k).map(|l| (l, v.clone())))
        .map(|(l, v)| {
            // Forge versions in mrpacks are just "47.3.0"; the maven needs "1.20.1-47.3.0".
            let v = if l == Loader::Forge && !v.contains('-') { format!("{mc}-{v}") } else { v };
            (l, Some(v))
        })
        .unwrap_or((Loader::Vanilla, None));
    Ok((mc, loader, lv))
}

fn side_name(kind: Kind) -> &'static str {
    if kind == Kind::Client { "client" } else { "server" }
}

/// Pack files for this side, as `/`-separated paths relative to the game folder.
fn pack_paths(index: &MrIndex, kind: Kind) -> Vec<(String, &MrFile)> {
    let side = side_name(kind);
    index
        .files
        .iter()
        .filter(|f| !f.env.as_ref().and_then(|e| e.get(side)).map(|s| s == "unsupported").unwrap_or(false))
        .filter_map(|f| safe_rel(&f.path).map(|r| (r.to_string_lossy().replace('\\', "/"), f)))
        .collect()
}

fn pack_files_path(inst: &Instance) -> PathBuf {
    inst.dir().join("modpack-files.json")
}

fn load_pack_files(inst: &Instance) -> Vec<String> {
    std::fs::read(pack_files_path(inst)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_pack_files(inst: &Instance, files: &[String]) -> Result<()> {
    std::fs::write(pack_files_path(inst), serde_json::to_vec_pretty(files)?)?;
    Ok(())
}

fn disabled_path(p: &Path) -> PathBuf {
    PathBuf::from(format!("{}.disabled", p.to_string_lossy()))
}

/// Files a player changes themselves: when switching pack versions, keep theirs.
fn is_player_file(rel: &Path) -> bool {
    let s = rel.to_string_lossy().replace('\\', "/");
    s.starts_with("saves/") || matches!(s.as_str(), "options.txt" | "servers.dat" | "server.properties" | "ops.json" | "whitelist.json")
}

/// Download a pack's files and apply its overrides into the instance.
/// With `update`, the player's own settings and worlds are left alone.
async fn apply_pack(inst: &Instance, zip: &mut zip::ZipArchive<std::fs::File>, index: &MrIndex, update: bool, rep: &Reporter) -> Result<Vec<String>> {
    let game = inst.game_dir();
    let files = pack_paths(index, inst.kind);
    let mut dls = Vec::new();
    let mut keep_disabled = Vec::new();
    for (rel, f) in &files {
        let path = game.join(rel);
        // A pack mod the player switched off stays off.
        let off = disabled_path(&path);
        if update && off.exists() {
            let _ = std::fs::remove_file(&off);
            keep_disabled.push(path.clone());
        }
        dls.push(Dl { urls: f.downloads.clone(), path, sha1: f.hashes.get("sha1").cloned(), size: Some(f.file_size).filter(|s| *s > 0) });
    }
    net::download_all(dls, rep, "Downloading modpack files", true).await?;
    for p in keep_disabled {
        let _ = std::fs::rename(&p, disabled_path(&p));
    }

    rep.stage("Applying overrides");
    let side_dir = format!("{}-overrides/", side_name(inst.kind));
    // `overrides/` first, then `<side>-overrides/` so they win (zip order isn't guaranteed).
    for pass in 0..2 {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name()?.to_string();
            let rel = if pass == 0 { name.strip_prefix("overrides/") } else { name.strip_prefix(side_dir.as_str()) };
            let Some(rel) = rel else { continue };
            if entry.is_dir() || rel.is_empty() {
                continue;
            }
            let Some(rel) = safe_rel(rel) else { continue };
            let dst = game.join(&rel);
            if update && dst.exists() && is_player_file(&rel) {
                continue;
            }
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::File::create(&dst)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(files.into_iter().map(|(r, _)| r).collect())
}

/// Create an instance from a `.mrpack` file.
pub async fn install_mrpack(path: &Path, kind: Kind, source: Option<String>, rep: &Reporter) -> Result<Instance> {
    rep.stage("Reading modpack");
    let (mut zip, index) = read_index(path)?;
    let (mc, loader, lv) = pack_target(&index)?;
    let name = if index.version_id.is_empty() { index.name.clone() } else { format!("{} {}", index.name, index.version_id) };
    let mut inst = Instance::new(&name, kind, &mc, loader, lv);
    inst.modpack = source;
    if !index.version_id.is_empty() {
        inst.modpack_version_name = Some(index.version_id.clone());
    }
    inst.save()?;
    let files = apply_pack(&inst, &mut zip, &index, false, rep).await?;
    save_pack_files(&inst, &files)?;
    rep.stage("Identifying the modpack's mods");
    match sync_installed(&inst).await {
        Ok(n) => rep.info(format!("Recognised {n} mods/packs from Modrinth")),
        Err(e) => rep.warn(format!("Couldn't identify the modpack's mods: {e:#}")),
    }
    rep.info(format!("Created instance “{}” ({} {})", inst.name, mc, loader.name()));
    Ok(inst)
}

/// What happened to the player's own content when a pack version changed.
#[derive(Default, Debug, Clone)]
pub struct PackChange {
    /// Moved to a version that fits the new Minecraft version.
    pub updated: Vec<String>,
    /// No compatible version exists, so they were switched off.
    pub disabled: Vec<String>,
}

/// Switch a modpack instance to another version of its pack, in place.
/// Worlds and the player's settings are kept. Content the player added
/// themselves is moved to a compatible version, or switched off if none exists.
pub async fn change_modpack_version(inst: &Instance, version_id: &str, rep: &Reporter) -> Result<(Instance, PackChange)> {
    let mut inst = inst.clone();
    let pack_id = inst.modpack.clone().ok_or_else(|| anyhow!("this instance wasn't installed from a Modrinth modpack"))?;
    rep.stage("Fetching modpack version");
    let v = get_version(version_id).await?;
    if v.project_id != pack_id {
        bail!("that version belongs to a different modpack");
    }
    let path = download_pack(&v).await?;
    let (mut zip, index) = read_index(&path)?;
    let (mc, loader, lv) = pack_target(&index)?;
    let game = inst.game_dir();
    let new_paths: HashSet<String> = pack_paths(&index, inst.kind).into_iter().map(|(r, _)| r).collect();

    rep.stage("Removing the old version's files");
    let remove = |p: &Path| {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(disabled_path(p));
    };
    for rel in load_pack_files(&inst) {
        if !new_paths.contains(&rel) {
            if let Some(r) = safe_rel(&rel) {
                remove(&game.join(r));
            }
        }
    }
    // Older copies of mods the new version ships under another file name
    // (also covers instances made before Lodestone tracked pack files).
    let hashes: Vec<&str> = index.files.iter().filter_map(|f| f.hashes.get("sha1").map(|s| s.as_str())).collect();
    let index_before = InstalledIndex::load(&inst);
    match versions_by_hash(&hashes).await {
        Ok(found) => {
            let pack_projects: HashSet<&str> = found.values().map(|v| v.project_id.as_str()).collect();
            let mut idx = InstalledIndex::load(&inst);
            for (pid, p) in &index_before.projects {
                if pack_projects.contains(pid.as_str()) && !new_paths.contains(&format!("{}/{}", p.folder, p.file)) {
                    remove(&inst.content_dir(&p.folder).join(&p.file));
                    idx.projects.remove(pid);
                }
            }
            idx.save(&inst)?;
        }
        Err(e) => rep.warn(format!("Couldn't check for older copies of the pack's mods: {e:#}")),
    }

    let files = apply_pack(&inst, &mut zip, &index, true, rep).await?;

    let (old_mc, old_loader) = (inst.mc_version.clone(), inst.loader);
    inst.mc_version = mc.clone();
    inst.loader = loader;
    inst.loader_version = lv;
    inst.version_id = None;
    inst.server.launch = None;
    if let Some(old) = inst.modpack_version_name.clone() {
        if let Some(base) = inst.name.strip_suffix(old.as_str()) {
            inst.name = format!("{base}{}", v.version_number);
        }
    }
    inst.modpack_version = Some(v.id.clone());
    inst.modpack_version_name = Some(v.version_number.clone());
    inst.save()?;
    save_pack_files(&inst, &files)?;

    rep.stage("Identifying the modpack's mods");
    if let Err(e) = sync_installed(&inst).await {
        rep.warn(format!("Couldn't identify the modpack's mods: {e:#}"));
    }

    let mut change = PackChange::default();
    if old_mc != inst.mc_version || old_loader != inst.loader {
        rep.stage("Checking your own mods");
        let pack: HashSet<String> = files.into_iter().collect();
        let idx = InstalledIndex::load(&inst);
        for (pid, p) in idx.projects {
            if pack.contains(&format!("{}/{}", p.folder, p.file)) {
                continue;
            }
            let file = inst.content_dir(&p.folder).join(&p.file);
            if !file.exists() && !disabled_path(&file).exists() {
                continue;
            }
            match compatible_versions(&inst, &pid).await {
                Ok(list) if list.iter().any(|x| x.version_number == p.version) => {}
                Ok(list) if !list.is_empty() => match install_project(&inst, &pid, Some(list[0].id.clone()), rep).await {
                    Ok(_) => change.updated.push(p.title.clone()),
                    Err(e) => rep.warn(format!("Couldn't update {}: {e:#}", p.title)),
                },
                Ok(_) => {
                    if file.exists() {
                        let _ = std::fs::rename(&file, disabled_path(&file));
                    }
                    change.disabled.push(p.title.clone());
                }
                Err(e) => rep.warn(format!("Couldn't check {}: {e:#}", p.title)),
            }
        }
    }
    rep.info(format!("Switched to {} {} ({} {})", index.name, v.version_number, mc, loader.name()));
    Ok((inst, change))
}

/// Download (and cache) a project icon; returns the local path.
pub async fn cached_icon(url: &str) -> Result<PathBuf> {
    let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("png");
    let path = paths().cache().join("icons").join(format!("{}.{ext}", net::sha1_bytes(url.as_bytes())));
    if !path.exists() {
        net::download_one(&Dl::new(url, &path)).await?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ver(games: &[&str], loaders: &[&str]) -> MVersion {
        MVersion {
            id: "x".into(),
            project_id: "p".into(),
            name: String::new(),
            version_number: "1".into(),
            game_versions: games.iter().map(|s| s.to_string()).collect(),
            loaders: loaders.iter().map(|s| s.to_string()).collect(),
            files: vec![],
            dependencies: vec![],
            date_published: String::new(),
            version_type: "release".into(),
        }
    }

    #[test]
    fn compatibility() {
        let mut inst = Instance::new("t", Kind::Client, "1.21", Loader::Fabric, None);
        assert!(is_compatible(&inst, &ver(&["1.21"], &["fabric"]), ContentType::Mod));
        assert!(!is_compatible(&inst, &ver(&["1.20.1"], &["fabric"]), ContentType::Mod));
        assert!(!is_compatible(&inst, &ver(&["1.21"], &["forge"]), ContentType::Mod));
        // Resource packs only care about the game version.
        assert!(is_compatible(&inst, &ver(&["1.21"], &["minecraft"]), ContentType::ResourcePack));
        // Quilt runs Fabric mods.
        inst.loader = Loader::Quilt;
        assert!(is_compatible(&inst, &ver(&["1.21"], &["fabric"]), ContentType::Mod));
    }

    /// End-to-end against a mock Modrinth: `LODESTONE_MODRINTH_API=http://127.0.0.1:PORT cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn change_pack_version() {
        let rep = Reporter::new("test");
        let inst = install_modpack("PACK", Some("P1".into()), Kind::Client, &rep).await.unwrap();
        assert_eq!(inst.mc_version, "1.20.1");
        assert_eq!(inst.name, "Pack 1.0");
        install_project(&inst, "U", None, &rep).await.unwrap();
        install_project(&inst, "W", None, &rep).await.unwrap();
        let game = inst.game_dir();
        std::fs::write(game.join("options.txt"), "mine").unwrap();
        std::fs::create_dir_all(game.join("saves/world")).unwrap();
        std::fs::write(game.join("saves/world/level.dat"), "w").unwrap();
        // The player switched a pack mod off.
        std::fs::rename(game.join("mods/b-1.0.jar"), game.join("mods/b-1.0.jar.disabled")).unwrap();

        let (inst, change) = change_modpack_version(&inst, "P2", &rep).await.unwrap();
        let mut mods: Vec<String> = std::fs::read_dir(game.join("mods")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        mods.sort();
        assert_eq!(mods, ["a-2.jar", "c.jar", "u-2.jar", "w.jar.disabled"]);
        assert_eq!(std::fs::read_to_string(game.join("options.txt")).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(game.join("config/x.txt")).unwrap(), "v2");
        assert!(game.join("saves/world/level.dat").exists());
        assert_eq!(inst.mc_version, "1.21");
        assert_eq!(inst.loader_version.as_deref(), Some("0.16.0"));
        assert_eq!(inst.name, "Pack 2.0");
        assert_eq!(change.updated, ["UserMod"]);
        assert_eq!(change.disabled, ["OldOnly"]);
        // A version that doesn't fit is refused.
        assert!(install_project(&inst, "W", Some("W1".into()), &rep).await.is_err());
        // Going back works too.
        let (inst, _) = change_modpack_version(&inst, "P1", &rep).await.unwrap();
        assert_eq!(inst.mc_version, "1.20.1");
        assert!(game.join("mods/a.jar").exists() && game.join("mods/b-1.0.jar").exists() && !game.join("mods/c.jar").exists());
        assert!(game.join("mods/u-1.jar").exists() && !game.join("mods/u-2.jar").exists());
    }
}
