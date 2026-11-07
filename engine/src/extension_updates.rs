//! Read-only gallery checks for local VS Code-family installations.
use crate::{
    Error, Result,
    http::{HttpClient, Transport},
    inventory::{self, Manager, Runner},
};
use semver::Version;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

const DEFAULT_PROFILE: &str = "__default__profile__";
const BATCH_SIZE: usize = 12;
const MAX_IDS: usize = 480;
const MICROSOFT: &str = "https://marketplace.visualstudio.com/_apis/public/gallery";
const CURSOR: &str = "https://marketplace.cursorapi.com/_apis/public/gallery";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub id: String,
    pub version: String,
    pub profile: String,
    pub root: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Available(String),
    NoNewerCompatibleRelease,
    Unverified(&'static str),
}

#[derive(Deserialize)]
struct Editor {
    #[serde(deserialize_with = "editor_version")]
    version: Version,
    platform: String,
    date: Option<String>,
    gallery: String,
    registries: Vec<Registry>,
}
fn editor_version<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Version, D::Error> {
    Version::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
}
#[derive(Deserialize)]
struct Registry {
    id: String,
    path: PathBuf,
}
#[derive(Deserialize)]
struct Entry {
    identifier: Identifier,
    version: String,
    metadata: Option<Metadata>,
    location: Option<Location>,
}
#[derive(Deserialize)]
struct Identifier {
    id: String,
    uuid: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    id: Option<String>,
    source: Option<String>,
    pre_release: Option<bool>,
    private: Option<bool>,
}
#[derive(Deserialize)]
struct Location {
    scheme: String,
    path: String,
    authority: Option<String>,
}

fn registry(path: &Path) -> Result<Vec<Entry>> {
    const LIMIT: u64 = 8 * 1024 * 1024;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Source("extension registry exceeds 8 MiB".into()));
    }
    Ok(serde_json::from_slice(
        bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes),
    )?)
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn matches_root(entry: &Entry, root: &Path) -> bool {
    let Some(location) = entry.location.as_ref() else {
        return false;
    };
    if location.scheme != "file" || location.authority.as_deref().is_some_and(|s| !s.is_empty()) {
        return false;
    }
    let mut url = url::Url::parse("file:///").unwrap();
    url.set_path(&location.path);
    url.to_file_path().is_ok_and(|path| {
        root.to_str().zip(path.to_str()).is_some_and(|(a, b)| {
            a.trim_end_matches(['/', '\\'])
                .eq_ignore_ascii_case(b.trim_end_matches(['/', '\\']))
        })
    })
}

fn membership<'a>(
    target: &Installation,
    profile: &'a [Entry],
    default: &'a [Entry],
) -> Option<(&'a str, bool)> {
    let matches = |entry: &&Entry| entry.identifier.id.eq_ignore_ascii_case(&target.id);
    let mut found = profile.iter().filter(matches);
    let entry = if let Some(entry) = found.next() {
        if found.next().is_some()
            || target
                .root
                .as_deref()
                .is_some_and(|root| !matches_root(entry, root))
        {
            return None;
        }
        entry
    } else {
        // A non-default profile can inherit a default extension. Use the saved
        // artifact location as well as its ID and version to establish the match.
        let root = target.root.as_deref()?;
        let mut inherited = default.iter().filter(matches);
        let entry = inherited.next()?;
        if inherited.next().is_some() || !matches_root(entry, root) {
            return None;
        }
        entry
    };
    let metadata = entry.metadata.as_ref()?;
    let id = entry
        .identifier
        .uuid
        .as_deref()
        .or(metadata.id.as_deref())?;
    (entry.version == target.version
        && metadata.private != Some(true)
        && metadata.source.as_deref() == Some("gallery")
        && uuid(id))
    .then_some((id, metadata.pre_release.unwrap_or(false)))
}

// VS Code engines use exact/caret/minimum versions, x wildcards and optional
// build-date gates, not a general npm range. Unsupported inputs stay unverified.
fn engine_matches(range: &str, editor: &Editor) -> Option<bool> {
    let range = range.trim();
    if range == "*" {
        return Some(true);
    }
    let (minimum, caret, range) = if let Some(v) = range.strip_prefix(">=") {
        (true, false, v)
    } else if let Some(v) = range.strip_prefix('^') {
        (false, true, v)
    } else {
        (false, false, range)
    };
    let (core, gate) = range
        .split_once('-')
        .map_or((range, None), |(v, d)| (v, Some(d)));
    let mut segments = core.split('.');
    let mut base = [0; 3];
    let mut exact = [true; 3];
    for i in 0..3 {
        match segments.next()? {
            "x" => exact[i] = false,
            n if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
                base[i] = n.parse().ok()?
            }
            _ => return None,
        }
    }
    if segments.next().is_some() {
        return None;
    }
    if caret {
        exact[2] = false;
        if base[0] != 0 {
            exact[1] = false;
        }
    }
    let actual = [
        editor.version.major,
        editor.version.minor,
        editor.version.patch,
    ];
    if !minimum && actual[0] == 1 && base[0] == 0 && exact.contains(&false) {
        base = [1, 0, 0];
        exact = [true, false, false];
    }
    let date_matches = || {
        let Some(gate) = gate else { return Some(true) };
        if !matches!(gate.len(), 8 | 10 | 12) || !gate.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let day = chrono::NaiveDate::parse_from_str(&gate[..8], "%Y%m%d").ok()?;
        let number = |a, b| gate.get(a..b).unwrap_or("0").parse::<u32>().ok();
        let cutoff = day
            .and_hms_opt(number(8, 10)?, number(10, 12)?, 0)?
            .and_utc();
        let build = chrono::DateTime::parse_from_rfc3339(editor.date.as_deref()?).ok()?;
        Some(build >= cutoff)
    };
    if minimum {
        return if actual[..2] == base[..2] {
            Some(actual[2] >= base[2] && date_matches()?)
        } else {
            Some(actual > base)
        };
    }
    for i in 0..3 {
        if actual[i] != base[i] {
            return Some(actual[i] > base[i] && !exact[i]);
        }
    }
    date_matches()
}

const MISSING: Status =
    Status::Unverified("The installed profile or gallery identity could not be verified");

fn property<'a>(version: &'a Value, key: &str) -> Option<&'a str> {
    version["properties"]
        .as_array()?
        .iter()
        .find(|p| p["key"] == key)?["value"]
        .as_str()
}
fn blocked(record: &Value) -> bool {
    !record["blocking"].is_null()
        || record["flags"].as_str().is_some_and(|flags| {
            flags
                .split([',', ' '])
                .any(|flag| matches!(flag, "blocked" | "unpublished"))
        })
}
fn compare(ext: &Value, editor: &Editor, target: &Installation, preferred_preview: bool) -> Status {
    let Ok(installed) = Version::parse(&target.version) else {
        return MISSING;
    };
    let Some(versions) = ext["versions"].as_array().filter(|v| !v.is_empty()) else {
        return Status::Unverified("No version metadata from the editor gallery");
    };
    if blocked(ext) {
        return Status::Unverified("This extension is blocked or unpublished in the gallery");
    }
    let mut best: Option<Version> = None;
    let mut verified_installed = false;
    let mut incomplete = false;
    for v in versions {
        let Some(version) = v["version"].as_str().and_then(|s| Version::parse(s).ok()) else {
            incomplete = true;
            continue;
        };
        if blocked(v) || (version.cmp_precedence(&installed).is_lt()) {
            continue;
        }
        if let Some(platform) = v["targetPlatform"].as_str()
            && !matches!(platform, "" | "universal")
            && platform != editor.platform
        {
            continue;
        }
        let preview = property(v, "Microsoft.VisualStudio.Code.PreRelease")
            .map_or(!version.pre.is_empty(), |v| v.eq_ignore_ascii_case("true"));
        if preview && !preferred_preview {
            continue;
        }
        verified_installed |= version == installed;
        if !version.cmp_precedence(&installed).is_gt() {
            continue;
        }
        // Proposed API exceptions and private policy are not proof of installability.
        match property(v, "Microsoft.VisualStudio.Code.Engine")
            .and_then(|range| engine_matches(range, editor))
        {
            Some(true)
                if property(v, "Microsoft.VisualStudio.Code.EnabledApiProposals")
                    .is_none_or(|p| p.trim().is_empty()) =>
            {
                if best
                    .as_ref()
                    .is_none_or(|old| version.cmp_precedence(old).is_gt())
                {
                    best = Some(version);
                }
            }
            Some(false) => {}
            _ => incomplete = true,
        }
    }
    if let Some(best) = best {
        Status::Available(best.to_string())
    } else if verified_installed && !incomplete {
        Status::NoNewerCompatibleRelease
    } else {
        Status::Unverified("The editor gallery did not provide enough compatible version metadata")
    }
}

fn check_editor(
    editor: &Editor,
    targets: &[Installation],
    query: impl Fn(&[&str]) -> Result<Value>,
) -> Vec<Status> {
    let paths: HashMap<_, _> = editor
        .registries
        .iter()
        .map(|registry| (registry.id.as_str(), registry.path.as_path()))
        .collect();
    let registries: HashMap<_, _> = targets
        .iter()
        .map(|target| target.profile.as_str())
        .chain(std::iter::once(DEFAULT_PROFILE))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|id| (id, paths.get(id).and_then(|path| registry(path).ok())))
        .collect();
    let default = registries
        .get(DEFAULT_PROFILE)
        .and_then(Option::as_deref)
        .unwrap_or_default();
    let mut groups: BTreeMap<String, Vec<_>> = BTreeMap::new();
    let mut results = vec![MISSING; targets.len()];
    for (index, target) in targets.iter().enumerate() {
        if let Some(profile) = registries
            .get(target.profile.as_str())
            .and_then(Option::as_deref)
            && let Some((uuid, preview)) = membership(target, profile, default)
        {
            groups
                .entry(target.id.to_ascii_lowercase())
                .or_default()
                .push((index, uuid, preview));
        }
    }
    let keys: Vec<_> = groups.keys().map(String::as_str).collect();
    let mut unavailable = false;
    for batch in keys.chunks(BATCH_SIZE) {
        let records = if unavailable { None } else { query(batch).ok() };
        let records = records.as_ref().and_then(|data| data["results"].as_array());
        unavailable |= records.is_none();
        for id in batch {
            for &(index, uuid, preview) in &groups[*id] {
                let Some(records) = records else {
                    results[index] =
                        Status::Unverified("The editor gallery request was unavailable");
                    continue;
                };
                let mut matches = records
                    .iter()
                    .filter_map(|r| r["extensions"].as_array())
                    .flatten()
                    .filter(|ext| {
                        ext["extensionId"]
                            .as_str()
                            .is_some_and(|s| s.eq_ignore_ascii_case(uuid))
                            && ext["publisher"]["publisherName"]
                                .as_str()
                                .zip(ext["extensionName"].as_str())
                                .is_some_and(|(p, name)| {
                                    format!("{p}.{name}").eq_ignore_ascii_case(id)
                                })
                    });
                if let Some(ext) = matches.next()
                    && matches.next().is_none()
                {
                    results[index] = compare(ext, editor, &targets[index], preview);
                }
            }
        }
    }
    results
}

pub fn check(
    manager: Manager,
    targets: &[Installation],
    runner: &impl Runner,
    client: &HttpClient<impl Transport>,
) -> Result<Vec<Status>> {
    let service = match manager {
        Manager::Vscode | Manager::VscodeInsiders => MICROSOFT,
        Manager::Cursor => CURSOR,
        _ => return Err(Error::Query("Select an editor provider".into())),
    };
    if targets
        .iter()
        .map(|t| t.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .len()
        > MAX_IDS
    {
        return Err(Error::Query(
            "Too many extension IDs; narrow the editor or profile filter".into(),
        ));
    }
    let output = runner.run(&format!(
        "$editorUpdatesOnly=1;\n{}",
        inventory::script(manager, false)
    ))?;
    if output.code != 0 {
        return Err(Error::Source(
            "Could not read the local editor metadata".into(),
        ));
    }
    let editor: Editor = serde_json::from_str(&output.stdout).map_err(|_| {
        Error::Source(
            "The installed editor version, platform or gallery could not be verified".into(),
        )
    })?;
    if editor.gallery.trim_end_matches('/') != service {
        return Err(Error::Source(
            "The installed editor uses an unsupported gallery".into(),
        ));
    }
    Ok(check_editor(&editor, targets, |batch| {
        let doc = client.extension_versions(service, batch, manager == Manager::Cursor)?;
        if !matches!(
            doc.evidence.cache.as_str(),
            "network" | "revalidated_cache" | "fresh_cache"
        ) {
            return Err(Error::Source(
                "Only stale gallery results are available".into(),
            ));
        }
        Ok(serde_json::from_str(&doc.body)?)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    const ID: &str = "a2c1ac6b-acf7-49c0-bea3-096e823eafbb";

    fn editor() -> Editor {
        Editor {
            version: Version::parse("1.125.0-insider").unwrap(),
            platform: "win32-x64".into(),
            date: Some("2026-11-20T12:00:00Z".into()),
            gallery: MICROSOFT.into(),
            registries: vec![],
        }
    }

    fn version(ver: &str, engine: &str, platform: &str, preview: bool) -> Value {
        json!({"version":ver, "targetPlatform":platform, "properties":[
            {"key":"Microsoft.VisualStudio.Code.Engine","value":engine},
            {"key":"Microsoft.VisualStudio.Code.PreRelease","value":preview.to_string()}
        ]})
    }

    #[test]
    fn engine_ranges_follow_editor_not_generic_semver() {
        let editor = editor();
        for (range, expected) in [
            ("*", Some(true)),
            ("^1.125.0", Some(true)),
            ("^1.126.0", Some(false)),
            ("^0.10.0", Some(true)),
            ("1.124.x", Some(false)),
            ("1.125.x", Some(true)),
            (">=1.125.0-202611201230", Some(false)),
            (">=1.125.0-2026112011", Some(true)),
            ("^1.125.0 || ^2.0.0", None),
        ] {
            assert_eq!(engine_matches(range, &editor), expected, "{range}");
        }
    }

    #[test]
    fn deduplicates_requests_and_uses_the_profile_channel_and_platform() {
        let dir = tempfile::tempdir().unwrap();
        let mut editor = editor();
        for (profile, preview) in [(DEFAULT_PROFILE, false), ("work", true)] {
            let path = dir.path().join(format!("{profile}.json"));
            std::fs::write(
                &path,
                json!([
                    {"identifier":{"id":"vendor.tool","uuid":ID},"version":"1.0.0",
                     "metadata":{"source":"gallery","preRelease":preview}},
                    {"identifier":{"id":"vendor.private","uuid":ID},"version":"1.0.0",
                     "metadata":{"source":"gallery","private":true}}
                ])
                .to_string(),
            )
            .unwrap();
            editor.registries.push(Registry {
                id: profile.into(),
                path,
            });
        }
        let targets: Vec<_> = [
            (DEFAULT_PROFILE, "vendor.tool", "1.0.0"),
            ("work", "vendor.tool", "1.0.0"),
            ("work", "vendor.private", "1.0.0"),
            ("work", "vendor.tool", "0.9.0"),
        ]
        .into_iter()
        .map(|(profile, id, version)| Installation {
            id: id.into(),
            version: version.into(),
            profile: profile.into(),
            root: None,
        })
        .collect();
        let calls = RefCell::new(Vec::new());
        let status = check_editor(&editor, &targets, |ids| {
            calls
                .borrow_mut()
                .push(ids.iter().map(|id| id.to_string()).collect::<Vec<_>>());
            let mut blocked = version("5.0.0", "^1.100.0", "universal", false);
            blocked["flags"] = json!("validated,blocked");
            Ok(json!({"results":[{"extensions":[
                {"extensionId":"wrong-id","publisher":{"publisherName":"vendor"},"extensionName":"tool",
                    "versions":[version("99.0.0","*","universal",false)]},
                {"extensionId":ID,"publisher":{"publisherName":"vendor"},"extensionName":"tool",
                    "versions":[blocked,version("4.0.0","^1.126.0","universal",false),
                        version("3.0.0","^1.100.0","win32-arm64",false),
                        version("2.1.0","^1.100.0","win32-x64",true),
                        version("2.0.0","^1.100.0","universal",false),
                        version("1.0.0","^1.100.0","universal",false)]}
            ]}]}))
        });
        assert_eq!(calls.into_inner(), [vec!["vendor.tool".to_owned()]]);
        assert_eq!(
            status,
            [
                Status::Available("2.0.0".into()),
                Status::Available("2.1.0".into()),
                MISSING,
                MISSING
            ]
        );
    }

    #[test]
    fn incomplete_metadata_never_means_current() {
        let editor = editor();
        let target = Installation {
            id: "vendor.tool".into(),
            version: "1.0.0".into(),
            profile: DEFAULT_PROFILE.into(),
            root: None,
        };
        let data =
            json!({"versions":[{"version":"2.0.0"}, version("1.0.0","*","universal",false)]});
        assert!(matches!(
            compare(&data, &editor, &target, false),
            Status::Unverified(_)
        ));
        let data = json!({"versions":[version("2.0.0","^1.126.0","universal",false),
            version("1.0.0","*","universal",false)]});
        assert_eq!(
            compare(&data, &editor, &target, false),
            Status::NoNewerCompatibleRelease
        );
    }
}
