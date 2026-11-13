//! Serde types for Mojang's launcher metadata (version manifest + version
//! JSON). The same format is used by Fabric/Quilt/Forge/NeoForge profiles.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "inheritsFrom", default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(rename = "mainClass", default, skip_serializing_if = "Option::is_none")]
    pub main_class: Option<String>,
    #[serde(rename = "minecraftArguments", default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(rename = "assetIndex", default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<HashMap<String, Download>>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(rename = "javaVersion", default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<String>,
    #[serde(rename = "releaseTime", default, skip_serializing_if = "Option::is_none")]
    pub release_time: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Arg>,
    #[serde(default)]
    pub jvm: Vec<Arg>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    Ruled { rules: Vec<Rule>, value: StrOrVec },
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum StrOrVec {
    One(String),
    Many(Vec<String>),
}

impl StrOrVec {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            StrOrVec::One(s) => vec![s.clone()],
            StrOrVec::Many(v) => v.clone(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Library {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<LibDownloads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<Rule>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct LibDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Download {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AssetIndexRef {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct JavaVersion {
    #[serde(default)]
    pub component: Option<String>,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingEntry>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LoggingEntry {
    pub argument: String,
    pub file: LogFile,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LogFile {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Deserialize, Debug)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Deserialize, Debug)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// Merge a child profile (e.g. Fabric) on top of its parent (vanilla).
pub fn merge(child: VersionJson, parent: VersionJson) -> VersionJson {
    let arguments = match (parent.arguments, child.arguments) {
        (Some(p), Some(c)) => Some(Arguments {
            game: p.game.into_iter().chain(c.game).collect(),
            jvm: p.jvm.into_iter().chain(c.jvm).collect(),
        }),
        (p, c) => c.or(p),
    };
    let mut libraries = child.libraries;
    libraries.extend(parent.libraries);
    VersionJson {
        id: child.id,
        inherits_from: None,
        main_class: child.main_class.or(parent.main_class),
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        arguments,
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        downloads: child.downloads.or(parent.downloads),
        libraries,
        java_version: child.java_version.or(parent.java_version),
        logging: child.logging.or(parent.logging),
        kind: child.kind.or(parent.kind),
        jar: child.jar.or(parent.jar),
        release_time: child.release_time.or(parent.release_time),
    }
}
