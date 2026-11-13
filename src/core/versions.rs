//! Version manifest, version JSON resolution (`inheritsFrom`) and the client
//! install pipeline: client jar, libraries, natives, assets, log config.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

use super::model::{AssetIndex, ManifestVersion, VersionJson, VersionManifest, merge};
use super::net::{self, Dl};
use super::paths::paths;
use super::rules::{self, Env};
use super::session::Reporter;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub async fn manifest() -> Result<VersionManifest> {
    net::cached_json(MANIFEST_URL, "version_manifest_v2.json").await
}

pub fn manifest_entry<'a>(m: &'a VersionManifest, id: &str) -> Option<&'a ManifestVersion> {
    m.versions.iter().find(|v| v.id == id)
}

/// Load a version JSON from disk, downloading it from the manifest if needed.
pub async fn ensure_json(id: &str) -> Result<VersionJson> {
    let path = paths().version_json(id);
    if let Ok(bytes) = std::fs::read(&path) {
        return serde_json::from_slice(&bytes).with_context(|| format!("bad version file {}", path.display()));
    }
    let m = manifest().await?;
    let entry = manifest_entry(&m, id).ok_or_else(|| anyhow!("unknown Minecraft version {id}"))?;
    let bytes = net::get_bytes(&entry.url).await?;
    if let Some(expected) = &entry.sha1 {
        if !net::sha1_bytes(&bytes).eq_ignore_ascii_case(expected) {
            return Err(anyhow!("version JSON checksum mismatch for {id}"));
        }
    }
    std::fs::create_dir_all(paths().version_dir(id))?;
    std::fs::write(&path, &bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save_json(json: &VersionJson) -> Result<()> {
    std::fs::create_dir_all(paths().version_dir(&json.id))?;
    std::fs::write(paths().version_json(&json.id), serde_json::to_vec_pretty(json)?)?;
    Ok(())
}

/// A fully merged version, ready to install/launch.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub json: VersionJson,
    /// Version whose `.jar` is the game jar (the vanilla parent).
    pub jar_id: String,
    /// The vanilla Minecraft version at the bottom of the chain.
    pub mc_version: String,
}

pub async fn resolve(id: &str) -> Result<Resolved> {
    let mut chain = vec![ensure_json(id).await?];
    while let Some(parent) = chain.last().unwrap().inherits_from.clone() {
        if chain.len() > 8 {
            return Err(anyhow!("inheritsFrom chain too deep"));
        }
        chain.push(ensure_json(&parent).await?);
    }
    let base = chain.pop().unwrap();
    let mc_version = base.id.clone();
    let mut jar_id = base.jar.clone().unwrap_or_else(|| base.id.clone());
    let mut json = base;
    while let Some(child) = chain.pop() {
        if let Some(j) = &child.jar {
            jar_id = j.clone();
        }
        json = merge(child, json);
    }
    Ok(Resolved { json, jar_id, mc_version })
}

pub fn legacy_natives(json: &VersionJson) -> bool {
    json.libraries.iter().any(|l| l.natives.is_some())
}

/// Download everything needed to launch `res` with the given environment.
pub async fn install_client_files(res: &Resolved, env: &Env, rep: &Reporter) -> Result<()> {
    let p = paths();
    let mut files: Vec<Dl> = Vec::new();

    // Client jar
    let jar_path = p.version_jar(&res.jar_id);
    if let Some(client) = res.json.downloads.as_ref().and_then(|d| d.get("client")) {
        files.push(Dl::new(&client.url, &jar_path).sha1(client.sha1.clone()).size(client.size));
    } else if !jar_path.exists() {
        return Err(anyhow!("version {} has no client download", res.jar_id));
    }

    // Libraries (+ legacy natives)
    for lib in &res.json.libraries {
        if !rules::library_wanted(lib, env) {
            continue;
        }
        if let Some(a) = rules::main_artifact(lib) {
            if !a.urls.is_empty() {
                files.push(Dl {
                    urls: a.urls,
                    path: p.libraries().join(&a.rel),
                    sha1: a.sha1,
                    size: a.size,
                });
            }
        }
        if let Some(n) = rules::native_artifact(lib, env) {
            files.push(Dl { urls: n.urls, path: p.libraries().join(&n.rel), sha1: n.sha1, size: n.size });
        }
    }

    // Logging config
    if let Some(log) = res.json.logging.as_ref().and_then(|l| l.client.as_ref()) {
        files.push(
            Dl::new(&log.file.url, p.assets().join("log_configs").join(&log.file.id))
                .sha1(log.file.sha1.clone()),
        );
    }

    net::download_all(files, rep, "Downloading game files", true).await?;
    install_assets(res, rep).await?;
    Ok(())
}

async fn install_assets(res: &Resolved, rep: &Reporter) -> Result<()> {
    let Some(idx) = &res.json.asset_index else { return Ok(()) };
    let p = paths();
    let index_path = p.assets().join("indexes").join(format!("{}.json", idx.id));
    net::download_all(
        vec![Dl::new(&idx.url, &index_path).sha1(idx.sha1.clone())],
        rep,
        "Asset index",
        true,
    )
    .await?;
    let index: AssetIndex = serde_json::from_slice(&std::fs::read(&index_path)?)?;
    let objects = p.assets().join("objects");
    let files: Vec<Dl> = index
        .objects
        .values()
        .map(|o| {
            let sub = &o.hash[..2];
            Dl::new(
                format!("https://resources.download.minecraft.net/{sub}/{}", o.hash),
                objects.join(sub).join(&o.hash),
            )
            .sha1(Some(o.hash.clone()))
            .size(Some(o.size))
        })
        .collect();
    net::download_all(files, rep, "Downloading assets", false).await?;

    // Very old versions read assets from a "virtual" folder with real names.
    if index.is_virtual || index.map_to_resources {
        let target = virtual_assets_dir(&idx.id);
        for (name, o) in &index.objects {
            let dst = target.join(name);
            if !dst.exists() {
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(objects.join(&o.hash[..2]).join(&o.hash), &dst)?;
            }
        }
    }
    Ok(())
}

pub fn virtual_assets_dir(index_id: &str) -> PathBuf {
    paths().assets().join("virtual").join(index_id)
}

/// Copies legacy `map_to_resources` assets into the instance's game dir.
pub fn copy_resources_if_needed(res: &Resolved, game_dir: &std::path::Path) -> Result<()> {
    let Some(idx) = &res.json.asset_index else { return Ok(()) };
    let index_path = paths().assets().join("indexes").join(format!("{}.json", idx.id));
    let Ok(bytes) = std::fs::read(&index_path) else { return Ok(()) };
    let index: AssetIndex = serde_json::from_slice(&bytes)?;
    if !index.map_to_resources {
        return Ok(());
    }
    let src = virtual_assets_dir(&idx.id);
    let dst_root = game_dir.join("resources");
    for name in index.objects.keys() {
        let dst = dst_root.join(name);
        if !dst.exists() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::copy(src.join(name), &dst);
        }
    }
    Ok(())
}

/// Vanilla server jar for a Minecraft version.
pub async fn server_jar_download(mc: &str) -> Result<Dl> {
    let json = ensure_json(mc).await?;
    let server = json
        .downloads
        .as_ref()
        .and_then(|d| d.get("server"))
        .ok_or_else(|| anyhow!("Minecraft {mc} has no official server download"))?;
    Ok(Dl::new(&server.url, paths().cache().join("servers").join(format!("{mc}.jar")))
        .sha1(server.sha1.clone())
        .size(server.size))
}
