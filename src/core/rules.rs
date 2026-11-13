//! Evaluation of the `rules` blocks in version JSON, plus helpers to turn
//! library coordinates into files and URLs.

use std::collections::HashMap;

use super::model::{Library, Rule};

#[derive(Clone, Debug)]
pub struct Env {
    /// "osx", "windows" or "linux" (Mojang's names).
    pub os: &'static str,
    /// "x86_64", "aarch64" or "x86" — the architecture of the Java we run.
    pub arch: String,
    pub os_version: String,
    pub features: HashMap<String, bool>,
}

impl Env {
    pub fn host() -> Env {
        let os = if cfg!(target_os = "macos") {
            "osx"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "linux"
        };
        let os_version = if cfg!(target_os = "windows") {
            // Mojang only ever matches "^10\\." on Windows (10 and 11 both report 10.0).
            "10.0".to_string()
        } else {
            sysinfo::System::os_version().unwrap_or_default()
        };
        Env {
            os,
            arch: std::env::consts::ARCH.to_string(),
            os_version,
            features: HashMap::new(),
        }
    }

    pub fn is_arm(&self) -> bool {
        self.arch == "aarch64"
    }
}

pub fn rules_allow(rules: &Option<Vec<Rule>>, env: &Env) -> bool {
    let Some(rules) = rules else { return true };
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, env) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, env: &Env) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != env.os {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            let ok = match arch.as_str() {
                "x86" => env.arch == "x86",
                "x86_64" | "amd64" => env.arch == "x86_64",
                "arm64" | "aarch64" => env.arch == "aarch64",
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        if let Some(version) = &os.version {
            match regex::Regex::new(version) {
                Ok(re) => {
                    if !re.is_match(&env.os_version) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
    }
    if let Some(features) = &rule.features {
        for (k, v) in features {
            if env.features.get(k).copied().unwrap_or(false) != *v {
                return false;
            }
        }
    }
    true
}

/// `group:artifact:version[:classifier][@ext]` -> `group/path/artifact/version/artifact-version[-classifier].ext`
pub fn maven_path(coord: &str) -> Option<String> {
    let (coord, ext) = match coord.split_once('@') {
        Some((c, e)) => (c, e),
        None => (coord, "jar"),
    };
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let (g, a, v) = (parts[0], parts[1], parts[2]);
    let file = match parts.get(3) {
        Some(c) => format!("{a}-{v}-{c}.{ext}"),
        None => format!("{a}-{v}.{ext}"),
    };
    Some(format!("{}/{a}/{v}/{file}", g.replace('.', "/")))
}

/// Key used to de-duplicate libraries on the classpath (ignores version).
pub fn lib_key(name: &str) -> String {
    let name = name.split('@').next().unwrap_or(name);
    let parts: Vec<&str> = name.split(':').collect();
    match parts.len() {
        0..=2 => name.to_string(),
        3 => format!("{}:{}", parts[0], parts[1]),
        _ => format!("{}:{}:{}", parts[0], parts[1], parts[3..].join(":")),
    }
}

#[derive(Clone, Debug)]
pub struct LibFile {
    pub rel: String,
    pub urls: Vec<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

fn fix_repo(url: &str) -> String {
    let url = url
        .replace("http://files.minecraftforge.net/maven/", "https://maven.minecraftforge.net/")
        .replace("https://files.minecraftforge.net/maven/", "https://maven.minecraftforge.net/");
    let url = url.replacen("http://", "https://", 1);
    if url.ends_with('/') { url } else { format!("{url}/") }
}

const FALLBACK_REPOS: &[&str] = &[
    "https://libraries.minecraft.net/",
    "https://maven.minecraftforge.net/",
    "https://repo1.maven.org/maven2/",
];

/// Main (classpath) artifact of a library, if it has one.
pub fn main_artifact(lib: &Library) -> Option<LibFile> {
    if let Some(d) = &lib.downloads {
        if let Some(a) = &d.artifact {
            let rel = a.path.clone().or_else(|| maven_path(&lib.name))?;
            let urls = if a.url.is_empty() { vec![] } else { vec![a.url.clone()] };
            return Some(LibFile { rel, urls, sha1: a.sha1.clone(), size: a.size });
        }
        if lib.natives.is_some() || d.classifiers.is_some() {
            return None;
        }
    }
    if lib.natives.is_some() {
        return None;
    }
    let rel = maven_path(&lib.name)?;
    let mut urls = Vec::new();
    match &lib.url {
        Some(base) if !base.is_empty() => {
            urls.push(format!("{}{rel}", fix_repo(base)));
            for repo in FALLBACK_REPOS {
                let u = format!("{repo}{rel}");
                if !urls.contains(&u) {
                    urls.push(u);
                }
            }
        }
        _ => urls.push(format!("https://libraries.minecraft.net/{rel}")),
    }
    Some(LibFile { rel, urls, sha1: lib.sha1.clone(), size: lib.size })
}

/// Legacy (pre-1.19) natives jar for this OS, to be extracted before launch.
pub fn native_artifact(lib: &Library, env: &Env) -> Option<LibFile> {
    let natives = lib.natives.as_ref()?;
    let classifier = natives
        .get(env.os)?
        .replace("${arch}", if env.arch == "x86" { "32" } else { "64" });
    if let Some(a) = lib
        .downloads
        .as_ref()
        .and_then(|d| d.classifiers.as_ref())
        .and_then(|c| c.get(&classifier))
    {
        let rel = a
            .path
            .clone()
            .or_else(|| maven_path(&format!("{}:{classifier}", lib.name)))?;
        return Some(LibFile { rel, urls: vec![a.url.clone()], sha1: a.sha1.clone(), size: a.size });
    }
    let rel = maven_path(&format!("{}:{classifier}", lib.name))?;
    let base = lib.url.as_deref().map(fix_repo).unwrap_or_else(|| "https://libraries.minecraft.net/".into());
    Some(LibFile { rel: rel.clone(), urls: vec![format!("{base}{rel}")], sha1: None, size: None })
}

/// Modern versions ship natives as separate libraries per architecture
/// (e.g. `natives-macos-arm64`). Skip the ones for other architectures.
pub fn classifier_arch_ok(name: &str, env: &Env) -> bool {
    let parts: Vec<&str> = name.split(':').collect();
    let Some(c) = parts.get(3) else { return true };
    let c = c.to_ascii_lowercase();
    if c.ends_with("arm64") || c.ends_with("aarch_64") || c.ends_with("aarch64") {
        return env.arch == "aarch64";
    }
    if c.ends_with("x86_64") || c.ends_with("-x64") {
        return env.arch == "x86_64";
    }
    if c.ends_with("-x86") {
        return env.arch == "x86";
    }
    true
}

pub fn library_wanted(lib: &Library, env: &Env) -> bool {
    rules_allow(&lib.rules, env) && classifier_arch_ok(&lib.name, env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.4.1:natives-macos-arm64").unwrap(),
            "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-macos-arm64.jar"
        );
        assert_eq!(
            maven_path("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap(),
            "de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip"
        );
        assert_eq!(lib_key("a.b:c:1.0"), "a.b:c");
        assert_eq!(lib_key("a.b:c:1.0:natives-linux"), "a.b:c:natives-linux");
    }
}
