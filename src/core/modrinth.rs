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

pub const API: &str = "https://api.modrinth.com/v2";
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
        .get(format!("{API}/search"))
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
    let resp = HTTP.get(format!("{API}/project/{id}/version")).query(&q).send().await?.error_for_status()?;
    Ok(resp.json().await?)
}

pub async fn get_version(id: &str) -> Result<MVersion> {
    net::get_json(&format!("{API}/version/{id}")).await
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
pub async fn install_project(inst: &Instance, project_id: &str, rep: &Reporter) -> Result<Vec<String>> {
    let mut index = InstalledIndex::load(inst);
    let mut queue: VecDeque<(String, Option<String>)> = VecDeque::from([(project_id.to_string(), None)]);
    let mut done: HashSet<String> = HashSet::new();
    let mut installed = Vec::new();

    while let Some((pid, version_id)) = queue.pop_front() {
        if !done.insert(pid.clone()) {
            continue;
        }
        if pid != project_id && index.has(inst, &pid) {
            continue;
        }
        let project: Project = net::get_json(&format!("{API}/project/{pid}")).await?;
        let ct = ContentType::from_api(&project.project_type);
        if ct == ContentType::Modpack {
            bail!("{} is a modpack — install it from the Modpacks tab", project.title);
        }
        let version = match version_id {
            Some(vid) => get_version(&vid).await?,
            None => {
                let loaders = if ct == ContentType::Mod { loader_filter(inst.loader) } else { vec![] };
                let list = project_versions(&project.id, &loaders, Some(&inst.mc_version)).await?;
                list.into_iter().next().ok_or_else(|| {
                    anyhow!("{} has no version for Minecraft {} {}", project.title, inst.mc_version, if ct == ContentType::Mod { inst.loader.name() } else { "" })
                })?
            }
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

/// Download the latest version of a Modrinth modpack and install it as a new instance.
pub async fn install_modpack(project_id: &str, kind: Kind, rep: &Reporter) -> Result<Instance> {
    rep.stage("Fetching modpack");
    let versions = project_versions(project_id, &[], None).await?;
    let v = versions.into_iter().next().ok_or_else(|| anyhow!("modpack has no versions"))?;
    let f = primary(&v).ok_or_else(|| anyhow!("modpack has no files"))?;
    let path = paths().cache().join("modpacks").join(&f.filename);
    net::download_one(&Dl::new(&f.url, &path).sha1(f.hashes.get("sha1").cloned())).await?;
    install_mrpack(&path, kind, Some(project_id.to_string()), rep).await
}

/// Create an instance from a `.mrpack` file.
pub async fn install_mrpack(path: &Path, kind: Kind, source: Option<String>, rep: &Reporter) -> Result<Instance> {
    rep.stage("Reading modpack");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let index: MrIndex = {
        let mut s = String::new();
        zip.by_name("modrinth.index.json").context("not a Modrinth modpack (no modrinth.index.json)")?.read_to_string(&mut s)?;
        serde_json::from_str(&s)?
    };
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

    let name = if index.version_id.is_empty() { index.name.clone() } else { format!("{} {}", index.name, index.version_id) };
    let mut inst = Instance::new(&name, kind, &mc, loader, lv);
    inst.modpack = source;
    inst.save()?;
    let game = inst.game_dir();
    let side = if kind == Kind::Client { "client" } else { "server" };

    let mut dls = Vec::new();
    for f in &index.files {
        if f.env.as_ref().and_then(|e| e.get(side)).map(|s| s == "unsupported").unwrap_or(false) {
            continue;
        }
        let Some(rel) = safe_rel(&f.path) else {
            rep.warn(format!("Skipping unsafe path in modpack: {}", f.path));
            continue;
        };
        dls.push(Dl {
            urls: f.downloads.clone(),
            path: game.join(rel),
            sha1: f.hashes.get("sha1").cloned(),
            size: Some(f.file_size).filter(|s| *s > 0),
        });
    }
    net::download_all(dls, rep, "Downloading modpack files", true).await?;

    rep.stage("Applying overrides");
    let side_dir = format!("{side}-overrides/");
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.to_string();
        let rel = name.strip_prefix("overrides/").or_else(|| name.strip_prefix(side_dir.as_str()));
        let Some(rel) = rel else { continue };
        if entry.is_dir() || rel.is_empty() {
            continue;
        }
        let Some(rel) = safe_rel(rel) else { continue };
        let dst = game.join(rel);
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut out = std::fs::File::create(&dst)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    // Overrides from `*-overrides/` must win over `overrides/`, and zip order isn't guaranteed.
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.to_string();
        if let Some(rel) = name.strip_prefix(side_dir.as_str()) {
            if entry.is_dir() || rel.is_empty() {
                continue;
            }
            if let Some(rel) = safe_rel(rel) {
                let mut out = std::fs::File::create(game.join(rel))?;
                std::io::copy(&mut entry, &mut out)?;
            }
        }
    }
    rep.info(format!("Created instance “{}” ({} {})", inst.name, mc, loader.name()));
    Ok(inst)
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
