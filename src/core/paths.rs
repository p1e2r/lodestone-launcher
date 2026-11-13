use std::path::PathBuf;
use std::sync::LazyLock;

/// All on-disk locations used by the launcher. Shared game files (versions,
/// libraries, assets, Java runtimes) are stored once and reused by every
/// instance; each instance gets its own folder under `instances/`.
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn instances(&self) -> PathBuf {
        self.root.join("instances")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }
    pub fn version_json(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.json"))
    }
    pub fn version_jar(&self, id: &str) -> PathBuf {
        self.version_dir(id).join(format!("{id}.jar"))
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
}

pub static PATHS: LazyLock<Paths> = LazyLock::new(|| {
    let root = std::env::var_os("LODESTONE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            directories::ProjectDirs::from("dev", "p1e2r", "Lodestone")
                .map(|d| d.data_dir().to_path_buf())
        })
        .unwrap_or_else(|| PathBuf::from(".lodestone"));
    let _ = std::fs::create_dir_all(&root);
    Paths { root }
});

pub fn paths() -> &'static Paths {
    &PATHS
}
