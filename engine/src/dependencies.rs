//! Read-only, bounded project inventories. Scanning never executes package tooling.
use crate::{http::Fetcher, source_discovery::Hints, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Npm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub manifest: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,
    pub ecosystem: Ecosystem,
    pub project: String,
    pub manifest: PathBuf,
    pub package: String,
    pub alias: String,
    pub requirement: String,
    pub kind: String,
    pub installed: Option<String>,
    pub root: Option<PathBuf>,
    pub repository: Option<String>,
    pub directory: Option<String>,
    pub homepage: Option<String>,
    pub public_registry: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scan {
    pub root: PathBuf,
    pub projects: Vec<Project>,
    pub dependencies: Vec<Dependency>,
    pub warnings: Vec<String>,
}

fn read(path: &Path) -> Result<Value> {
    let mut body = String::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut body)?;
    if body.len() > 1024 * 1024 {
        return Err(Error::Source("manifest exceeds 1 MiB".into()));
    }
    let value: Value = serde_json::from_str(&body)?;
    if !value.is_object() {
        return Err(Error::Source("package manifest must be an object".into()));
    }
    Ok(value)
}

fn confined(path: &Path, root: &Path) -> Option<PathBuf> {
    path.canonicalize().ok().filter(|p| p.starts_with(root))
}

pub fn package_name(name: &str) -> bool {
    if name.len() > 214 {
        return false;
    }
    let valid = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['.', '_'])
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    if let Some(scoped) = name.strip_prefix('@') {
        scoped
            .split_once('/')
            .is_some_and(|(scope, name)| valid(scope) && valid(name))
    } else {
        valid(name)
    }
}

fn declaration(alias: &str, spec: &str) -> (String, bool) {
    let (name, spec) = if let Some(spec) = spec.strip_prefix("npm:") {
        let end = spec.rfind('@').filter(|&i| i > 0).unwrap_or(spec.len());
        (&spec[..end], spec.get(end + 1..).unwrap_or("*"))
    } else {
        (alias, spec)
    };
    let registry = package_name(name)
        && !spec.is_empty()
        && !spec.contains([':', '/', '\\'])
        && !spec.starts_with(['.', '#']);
    (name.into(), registry)
}

fn metadata(dependency: &mut Dependency, value: &Value) {
    dependency.repository = value["repository"]
        .as_str()
        .or_else(|| value["repository"]["url"].as_str())
        .map(String::from);
    dependency.directory = value["repository"]["directory"].as_str().map(String::from);
    dependency.homepage = value["homepage"].as_str().map(String::from);
}

fn installed(dependency: &mut Dependency, root: &Path) {
    let Some(mut folder) = dependency.manifest.parent() else {
        return;
    };
    loop {
        let candidate = folder.join("node_modules").join(&dependency.alias);
        let present = match fs::symlink_metadata(&candidate) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return,
        };
        if present {
            if let Some(path) = confined(&candidate.join("package.json"), root)
                && let Ok(value) = read(&path)
                && value["name"] == dependency.package
                && let Some(version) = value["version"].as_str()
                && semver::Version::parse(version).is_ok()
            {
                dependency.installed = Some(version.into());
                dependency.root = path.parent().map(Path::to_path_buf);
                metadata(dependency, &value);
                if value["private"] == true {
                    dependency.public_registry = false;
                }
            }
            // An unreadable, external or metadata-less nearer package still shadows
            // an ancestor installation. Do not assign the ancestor's version to it.
            return;
        }
        if folder == root {
            break;
        }
        let Some(parent) = folder.parent().filter(|p| p.starts_with(root)) else {
            break;
        };
        folder = parent;
    }
}

pub fn scan(root: &Path) -> Result<Scan> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(Error::Query("choose a project folder".into()));
    }
    let mut out = Scan {
        root: root.clone(),
        projects: vec![],
        dependencies: vec![],
        warnings: vec![],
    };
    let mut pending = vec![(root.clone(), 0)];
    let mut manifests = vec![];
    let mut visited = 0;
    while let Some((folder, depth)) = pending.pop() {
        visited += 1;
        if visited > 20_000 || manifests.len() >= 512 {
            out.warnings.push(
                "Scan limit reached; choose a smaller project folder for complete coverage.".into(),
            );
            break;
        }
        if let Some(path) = confined(&folder.join("package.json"), &root) {
            match read(&path) {
                Ok(value) => manifests.push((path, value)),
                Err(error) => out.warnings.push(format!("{}: {error}", folder.display())),
            }
        }
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                out.warnings.push(format!("{}: {error}", folder.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    out.warnings.push(error.to_string());
                    continue;
                }
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || [
                    "node_modules",
                    "target",
                    "dist",
                    "build",
                    "coverage",
                    "vendor",
                    "venv",
                ]
                .contains(&name.as_ref())
            {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            // Do not traverse symlinks/junctions into another project or drive.
            if !kind.is_dir() || kind.is_symlink() {
                continue;
            }
            if depth >= 12 {
                out.warnings
                    .push(format!("Depth limit at {}", entry.path().display()));
                continue;
            }
            if let Some(path) = confined(&entry.path(), &root) {
                pending.push((path, depth + 1));
            }
        }
    }
    manifests.sort_by(|a, b| a.0.cmp(&b.0));
    let local_names: HashSet<_> = manifests
        .iter()
        .filter_map(|(_, v)| v["name"].as_str())
        .collect();
    for (path, value) in &manifests {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let name = value["name"].as_str().unwrap_or(&relative);
        out.projects.push(Project {
            name: name.into(),
            manifest: path.clone(),
        });
        for (field, kind) in [
            ("dependencies", "runtime"),
            ("devDependencies", "dev"),
            ("optionalDependencies", "optional"),
            ("peerDependencies", "peer"),
        ] {
            let Some(object) = value.get(field) else {
                continue;
            };
            let Some(object) = object.as_object() else {
                out.warnings
                    .push(format!("{relative}: {field} is not an object"));
                continue;
            };
            for (alias, spec) in object {
                if out.dependencies.len() >= 10_000 {
                    return Err(Error::Source(
                        "more than 10,000 dependency declarations; choose a smaller project folder"
                            .into(),
                    ));
                }
                let Some(spec) = spec.as_str().filter(|_| package_name(alias)) else {
                    out.warnings
                        .push(format!("{relative}: invalid dependency {alias}"));
                    continue;
                };
                let (package, registry) = declaration(alias, spec);
                let id = format!(
                    "dependency-{:x}",
                    Sha256::digest(format!("{}\0{alias}\0{kind}", path.display()))
                );
                let mut dependency = Dependency {
                    id,
                    ecosystem: Ecosystem::Npm,
                    project: format!("{name} / {relative}"),
                    manifest: path.clone(),
                    alias: alias.clone(),
                    requirement: spec.into(),
                    kind: kind.into(),
                    public_registry: registry && !local_names.contains(package.as_str()),
                    package,
                    installed: None,
                    root: None,
                    repository: None,
                    directory: None,
                    homepage: None,
                };
                installed(&mut dependency, &root);
                out.dependencies.push(dependency);
            }
        }
    }
    if out.projects.is_empty() {
        out.warnings
            .push("No package.json manifests found in the selected folder.".into());
    }
    Ok(out)
}

/// Public metadata lookups happen only when the user requests a package's notes.
pub fn notes(fetch: impl Fetcher, dependency: &Dependency, channel: Channel) -> Result<Report> {
    let mut dependency = dependency.clone();
    let mut metadata_evidence = vec![];
    let mut profile = known_package(&dependency.package)?.filter(|_| dependency.public_registry);
    if profile.is_none() {
        if dependency.public_registry && dependency.repository.is_none() {
            let mut url = url::Url::parse("https://registry.npmjs.org/").unwrap();
            url.path_segments_mut()
                .unwrap()
                .pop_if_empty()
                .push(&dependency.package)
                .push(dependency.installed.as_deref().unwrap_or("latest"));
            let doc = fetch.get(url.as_str())?;
            let data: Value = serde_json::from_str(&doc.body)?;
            if data["name"] != dependency.package
                || dependency
                    .installed
                    .as_ref()
                    .is_some_and(|version| data["version"] != *version)
            {
                return Err(Error::Source(
                    "registry package identity/version mismatch".into(),
                ));
            }
            metadata(&mut dependency, &data);
            metadata_evidence.push(doc.evidence);
        }
        let hints = Hints {
            manager: "npm".into(),
            source: "package.json".into(),
            id: dependency.package.clone(),
            installed: dependency.installed.clone(),
            repository: dependency
                .public_registry
                .then(|| dependency.repository.clone())
                .flatten(),
            directory: dependency
                .public_registry
                .then(|| dependency.directory.clone())
                .flatten(),
            homepage: dependency
                .public_registry
                .then(|| dependency.homepage.clone())
                .flatten(),
            release_notes: None,
            downloads: vec![],
            root: dependency.root.as_ref().map(|p| p.to_string_lossy().into()),
            references: vec![],
        };
        let found = source_discovery::discover(&fetch, &hints);
        if found.streams.is_empty() {
            return Err(Error::Source(format!(
                "No authored package changelog found. {}",
                found.attempts.join("; ")
            )));
        }
        profile = Some(Package {
            id: dependency.package.clone(),
            name: dependency.package.clone(),
            tracking: "track".into(),
            identity: "package_metadata".into(),
            alias_of: None,
            caveats: String::new(),
            reference_urls: vec![],
            streams: found.streams,
        });
    }
    let mut report = package_history(fetch, profile.unwrap(), channel)?;
    report.package = dependency.id;
    report.scope = format!(
        "{} / {} / declared {} / installed {}",
        dependency.project,
        dependency.kind,
        dependency.requirement,
        dependency.installed.as_deref().unwrap_or("unresolved")
    );
    report.evidence.extend(metadata_evidence);
    report.warnings.push("Package source history, not a compatible update candidate. Declared ranges are not installed versions; package managers and project code were not executed.".into());
    Ok(report)
}

/// Reviewed package sources, without registry requests or source discovery.
pub fn known_package(id: &str) -> Result<Option<Package>> {
    let known: Catalog = serde_json::from_str(include_str!("../data/package-sources.json"))?;
    Ok(known.packages.into_iter().find(|p| p.id == id))
}

pub(crate) fn package_history(
    fetch: impl Fetcher,
    mut profile: Package,
    channel: Channel,
) -> Result<Report> {
    for stream in &mut profile.streams {
        stream.channel = channel;
    }
    let id = profile.id.clone();
    let engine = Engine {
        catalog: Catalog {
            schema_version: 1,
            packages: vec![profile],
        },
        fetcher: fetch,
    };
    let mut page = engine.history(&id, None, None, false)?;
    let mut report = page.report;
    filter_channel(&mut report, channel);
    // A busy preview stream can occupy the first page; do not scan all history.
    for _ in 0..2 {
        if !report.notes.is_empty() {
            break;
        }
        let Some(cursor) = page.next.take() else {
            break;
        };
        match engine.history(&id, None, Some(cursor), false) {
            Ok(next) => {
                page = next;
                let mut next_report = page.report;
                filter_channel(&mut next_report, channel);
                report.notes = next_report.notes;
                report.evidence.extend(next_report.evidence);
                report.warnings.extend(next_report.warnings);
                report.attempts.extend(next_report.attempts);
            }
            Err(error) => {
                report
                    .warnings
                    .push(format!("Package history stopped: {error}"));
                break;
            }
        }
    }
    report.status = if report.notes.is_empty() {
        "no_matching_notes"
    } else if report.notes.iter().any(|n| n.authored_notes) {
        "partial"
    } else {
        "no_authored_notes"
    }
    .into();
    report.complete = false;
    report.warnings.push("At most three source pages are checked to find the selected channel; only one matching page is returned.".into());
    Ok(report)
}

fn filter_channel(report: &mut Report, channel: Channel) {
    let unknown = channel != Channel::All
        && report
            .evidence
            .iter()
            .any(|e| e.url.ends_with("/releases.atom"));
    report.notes.retain(|note| {
        !unknown
            && match channel {
                Channel::Stable => !note.preview,
                Channel::Preview => note.preview,
                Channel::All => true,
            }
    });
    if unknown {
        report.warnings.push("GitHub Atom cannot establish publication-channel flags; choose All to read its unverified snapshot.".into());
    }
}
