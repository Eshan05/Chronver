//! Personal public-package subscriptions, independent of projects or installations.
use crate::{http::Fetcher, source_discovery::Hints, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Npm,
    Cargo,
}
impl Ecosystem {
    pub fn id(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Cargo => "cargo",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    #[default]
    Core,
    Tooling,
}
impl Group {
    pub fn id(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Tooling => "tooling",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub ecosystem: Ecosystem,
    pub name: String,
    #[serde(default)]
    pub group: Group,
}
impl Entry {
    pub fn validate(&self) -> Result<()> {
        let valid = match self.ecosystem {
            Ecosystem::Npm => dependencies::package_name(&self.name),
            Ecosystem::Cargo => {
                !self.name.is_empty()
                    && self.name.len() <= 64
                    && self.name.as_bytes()[0].is_ascii_alphabetic()
                    && self
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Query(
                "Enter a package name, without a URL or version range".into(),
            ))
        }
    }
    pub fn id(&self) -> String {
        format!(
            "watch-{}-{:x}",
            self.ecosystem.id(),
            Sha256::digest(self.name.as_bytes())
        )
    }
    pub fn url(&self) -> String {
        match self.ecosystem {
            Ecosystem::Npm => format!("https://www.npmjs.com/package/{}", self.name),
            Ecosystem::Cargo => format!("https://crates.io/crates/{}", self.name),
        }
    }
}

fn validate(entries: &[Entry]) -> Result<()> {
    if entries.len() > 1000 {
        return Err(Error::Config("watchlist exceeds 1,000 entries".into()));
    }
    let mut ids = HashSet::new();
    for entry in entries {
        entry.validate()?;
        if !ids.insert(entry.id()) {
            return Err(Error::Config("duplicate watchlist package".into()));
        }
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = vec![];
    file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 {
        return Err(Error::Config("watchlist file exceeds 256 KiB".into()));
    }
    let entries: Vec<Entry> = serde_json::from_slice(&bytes)?;
    validate(&entries)?;
    Ok(entries)
}
pub fn save(path: &Path, entries: &[Entry]) -> Result<()> {
    validate(entries)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Config("watchlist parent missing".into()))?;
    let bytes = serde_json::to_vec_pretty(entries)?;
    if bytes.len() > 256 * 1024 {
        return Err(Error::Config("watchlist file exceeds 256 KiB".into()));
    }
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn notes(fetch: impl Fetcher, entry: &Entry, channel: Channel) -> Result<Report> {
    entry.validate()?;
    let mut registry = url::Url::parse(match entry.ecosystem {
        Ecosystem::Npm => "https://registry.npmjs.org/",
        Ecosystem::Cargo => "https://crates.io/api/v1/crates/",
    })
    .unwrap();
    registry
        .path_segments_mut()
        .unwrap()
        .pop_if_empty()
        .push(&entry.name);
    if entry.ecosystem == Ecosystem::Npm {
        registry.path_segments_mut().unwrap().push("latest");
    }
    let doc = fetch.get(registry.as_str())?;
    let value: Value = serde_json::from_str(&doc.body)?;
    let data = if entry.ecosystem == Ecosystem::Cargo {
        &value["crate"]
    } else {
        &value
    };
    if data[if entry.ecosystem == Ecosystem::Cargo {
        "id"
    } else {
        "name"
    }] != entry.name
    {
        return Err(Error::Source("registry package identity mismatch".into()));
    }
    let latest = data[if entry.ecosystem == Ecosystem::Cargo {
        "max_stable_version"
    } else {
        "version"
    }]
    .as_str()
    .filter(|v| semver::Version::parse(v).is_ok())
    .ok_or_else(|| Error::Source("registry published version missing or invalid".into()))?;
    let preview = !semver::Version::parse(latest).unwrap().pre.is_empty();
    let key = if entry.ecosystem == Ecosystem::Cargo {
        format!("cargo:{}", entry.name)
    } else {
        entry.name.clone()
    };
    let mut attempts = vec![];
    let mut profile = dependencies::known_package(&key)?;
    if profile.is_none() && entry.ecosystem == Ecosystem::Npm {
        let hints = Hints {
            manager: "npm".into(),
            source: "public package watchlist".into(),
            id: entry.name.clone(),
            installed: None,
            repository: data["repository"]
                .as_str()
                .or_else(|| data["repository"]["url"].as_str())
                .map(String::from),
            directory: data["repository"]["directory"].as_str().map(String::from),
            homepage: data["homepage"].as_str().map(String::from),
            release_notes: None,
            downloads: vec![],
            root: None,
            references: vec![],
        };
        let found = source_discovery::discover(&fetch, &hints);
        attempts = found.attempts;
        if !found.streams.is_empty() {
            profile = Some(Package {
                id: key,
                name: entry.name.clone(),
                tracking: "track".into(),
                identity: "metadata_discovered".into(),
                alias_of: None,
                caveats: String::new(),
                reference_urls: vec![],
                streams: found.streams,
            });
        }
    }
    let fetched = profile.map(|profile| dependencies::package_history(fetch, profile, channel));
    let mut report = match fetched {
        Some(Ok(report)) if !report.notes.is_empty() => report,
        result => {
            if let Some(Err(error)) = result {
                attempts.push(error.to_string());
            } else if let Some(Ok(report)) = result {
                attempts.extend(report.warnings);
            }
            Report {package:entry.id(),stream:"published".into(),scope:String::new(),basis:Basis::Version,status:"no_authored_notes".into(),complete:false,
                notes:if channel!=Channel::All && preview!=(channel==Channel::Preview) {vec![]} else {vec![Note{title:format!("Published {latest}"),version:Some(latest.into()),date:None,url:entry.url(),body:String::new(),authored_notes:false,preview}]},
                warnings:vec!["Registry publication metadata is available, but no authored changelog was retrieved. This is not an installed-version update candidate.".into()],evidence:vec![],attempts:vec![]}
        }
    };
    report.package = entry.id();
    report.scope = format!(
        "{} / {} / public release history",
        entry.ecosystem.id(),
        entry.name
    );
    report.complete = false;
    report.evidence.push(doc.evidence);
    report.warnings.extend(attempts.into_iter().take(12));
    report
        .warnings
        .push("Personal package watchlist: no project or installed version was inferred.".into());
    Ok(report)
}
