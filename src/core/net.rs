use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use tokio::io::AsyncWriteExt;

use super::session::Reporter;

pub const USER_AGENT: &str = concat!(
    "p1e2r/lodestone/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/p1e2r/lodestone)"
);

/// Tokio runtime that drives all networking and child processes.
pub static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .thread_name("lodestone-io")
        .build()
        .expect("tokio runtime")
});

pub static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .expect("http client")
});

pub async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T> {
    let resp = HTTP
        .get(url)
        .send()
        .await
        .with_context(|| format!("request to {url} failed"))?
        .error_for_status()?;
    let bytes = resp.bytes().await?;
    serde_json::from_slice(&bytes).with_context(|| format!("bad JSON from {url}"))
}

pub async fn get_text(url: &str) -> Result<String> {
    Ok(HTTP
        .get(url)
        .send()
        .await
        .with_context(|| format!("request to {url} failed"))?
        .error_for_status()?
        .text()
        .await?)
}

pub async fn get_bytes(url: &str) -> Result<Vec<u8>> {
    Ok(HTTP
        .get(url)
        .send()
        .await
        .with_context(|| format!("request to {url} failed"))?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

/// One file to download. `urls` are tried in order (mirrors / fallbacks).
#[derive(Clone, Debug)]
pub struct Dl {
    pub urls: Vec<String>,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

impl Dl {
    pub fn new(url: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            urls: vec![url.into()],
            path: path.into(),
            sha1: None,
            size: None,
        }
    }
    pub fn sha1(mut self, sha1: Option<String>) -> Self {
        self.sha1 = sha1.filter(|s| !s.is_empty());
        self
    }
    pub fn size(mut self, size: Option<u64>) -> Self {
        self.size = size.filter(|s| *s > 0);
        self
    }
}

pub fn sha1_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub fn sha1_bytes(data: &[u8]) -> String {
    hex::encode(Sha1::digest(data))
}

/// Is the file already present and valid? With `deep`, the SHA-1 is checked
/// too (otherwise only the size, which is enough for content-addressed files).
fn is_valid(d: &Dl, deep: bool) -> bool {
    let Ok(meta) = std::fs::metadata(&d.path) else {
        return false;
    };
    if let Some(size) = d.size {
        if meta.len() != size {
            return false;
        }
    }
    if deep {
        if let Some(expected) = &d.sha1 {
            return sha1_file(&d.path)
                .map(|h| h.eq_ignore_ascii_case(expected))
                .unwrap_or(false);
        }
    }
    true
}

/// Download many files concurrently with progress reporting. Files that are
/// already present and valid are skipped.
pub async fn download_all(
    items: Vec<Dl>,
    rep: &Reporter,
    label: &str,
    deep_verify: bool,
) -> Result<()> {
    // De-duplicate by destination path.
    let mut seen = std::collections::HashSet::new();
    let items: Vec<Dl> = items
        .into_iter()
        .filter(|d| !d.urls.is_empty() && seen.insert(d.path.clone()))
        .collect();

    let needed: Vec<Dl> = tokio::task::spawn_blocking(move || {
        items
            .into_iter()
            .filter(|d| !is_valid(d, deep_verify))
            .collect::<Vec<_>>()
    })
    .await?;

    if needed.is_empty() {
        return Ok(());
    }
    rep.stage(format!("{label} ({} files)", needed.len()));
    let total = needed.len() as u64;
    rep.progress(0, total);

    let mut errors = Vec::new();
    let mut stream = futures::stream::iter(needed)
        .map(|d| async move {
            let r = download_one(&d).await;
            (d, r)
        })
        .buffer_unordered(24);
    while let Some((d, r)) = stream.next().await {
        rep.inc();
        if let Err(e) = r {
            rep.warn(format!("Failed: {} ({e:#})", d.path.display()));
            errors.push(e);
        }
    }
    if let Some(e) = errors.into_iter().next() {
        bail!("{label}: some downloads failed: {e:#}");
    }
    Ok(())
}

pub async fn download_one(d: &Dl) -> Result<()> {
    if let Some(parent) = d.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut last_err = anyhow!("no URL");
    for attempt in 0..3 {
        for url in &d.urls {
            match try_download(url, d).await {
                Ok(()) => return Ok(()),
                Err(e) => last_err = e,
            }
        }
        tokio::time::sleep(Duration::from_millis(400 * (attempt + 1))).await;
    }
    Err(last_err)
}

async fn try_download(url: &str, d: &Dl) -> Result<()> {
    let resp = HTTP.get(url).send().await?.error_for_status()?;
    let tmp = d.path.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Sha1::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if let Some(expected) = &d.sha1 {
        let got = hex::encode(hasher.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("checksum mismatch for {url}");
        }
    }
    tokio::fs::rename(&tmp, &d.path).await?;
    Ok(())
}

/// Fetch JSON, caching it to disk so the launcher still works offline.
pub async fn cached_json<T: DeserializeOwned>(url: &str, cache_name: &str) -> Result<T> {
    let cache = super::paths::paths().cache().join(cache_name);
    match get_bytes(url).await {
        Ok(bytes) => {
            let parsed: T = serde_json::from_slice(&bytes)?;
            if let Some(p) = cache.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::write(&cache, &bytes);
            Ok(parsed)
        }
        Err(e) => {
            let bytes = std::fs::read(&cache).map_err(|_| e)?;
            Ok(serde_json::from_slice(&bytes)?)
        }
    }
}
