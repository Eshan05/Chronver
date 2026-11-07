use crate::{
    http::https,
    model::{Error, Result},
};
use chrono::{DateTime, Utc};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub const DEFAULT_CACHE_MIB: u32 = 32;
const MIB: u64 = 1024 * 1024;
pub(crate) const MAX_BODY: u64 = 8 * MIB;
const MAX_ENTRY: u64 = 2 * MAX_BODY;
const MAX_ENCODED: u64 = MAX_ENTRY + MIB;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub url: String,
    #[serde(default)]
    pub effective_url: Option<String>,
    pub fetched_at: DateTime<Utc>,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Serialize)]
pub struct CachePruneReport {
    pub apply: bool,
    pub limit_bytes: u64,
    pub cache_files: usize,
    pub cache_bytes: u64,
    pub planned_files: usize,
    pub planned_bytes: u64,
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub skipped_candidates: usize,
    pub errors: Vec<String>,
}

fn is_redirected(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    meta.file_type().is_symlink()
}

fn check_directory(directory: &Path) -> Result<bool> {
    match fs::symlink_metadata(directory) {
        Ok(meta) if meta.is_dir() && !is_redirected(&meta) => Ok(true),
        Ok(_) => Err(Error::Config("HTTP cache must be a real directory".into())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn lock(directory: &Path) -> Result<File> {
    let path = directory.join(".http-cache.lock");
    match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() || is_redirected(&meta) => {
            return Err(Error::Config("invalid HTTP cache lock file".into()));
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

fn decode(path: &Path, compressed: bool, meta: &Metadata) -> Option<Entry> {
    let max = if compressed { MAX_ENCODED } else { MAX_ENTRY };
    if !meta.is_file() || is_redirected(meta) || meta.len() > max {
        return None;
    }
    let file = File::open(path).ok()?.take(max + 1);
    let mut bytes = Vec::new();
    if compressed {
        GzDecoder::new(file)
            .take(MAX_ENTRY + 1)
            .read_to_end(&mut bytes)
            .ok()?;
    } else {
        let mut file = file;
        file.read_to_end(&mut bytes).ok()?;
    }
    if bytes.len() as u64 > MAX_ENTRY {
        return None;
    }
    let entry: Entry = serde_json::from_slice(&bytes).ok()?;
    (entry.body.len() as u64 <= MAX_BODY
        && https(&entry.url).is_ok()
        && entry
            .effective_url
            .as_deref()
            .is_none_or(|url| https(url).is_ok())
        && entry
            .headers
            .keys()
            .all(|key| matches!(key.as_str(), "etag" | "last-modified" | "link")))
    .then_some(entry)
}

fn paths(directory: &Path, key: &str) -> (PathBuf, PathBuf) {
    (
        directory.join(format!("{key}.json.gz")),
        directory.join(format!("{key}.json")),
    )
}

pub(crate) fn load(directory: &Path, key: &str, url: &str) -> Option<Entry> {
    if !check_directory(directory).ok()? {
        return None;
    }
    let (compressed, legacy) = paths(directory, key);
    [(legacy, false), (compressed, true)]
        .into_iter()
        .filter_map(|(path, gzip)| decode(&path, gzip, &fs::symlink_metadata(&path).ok()?))
        .filter(|entry| entry.url == url)
        .max_by_key(|entry| entry.fetched_at)
}

pub(crate) fn store(directory: &Path, key: &str, entry: &Entry) -> Result<()> {
    store_with_limit(directory, key, entry, u64::from(DEFAULT_CACHE_MIB) * MIB)
}

fn store_with_limit(directory: &Path, key: &str, entry: &Entry, limit: u64) -> Result<()> {
    let bytes = serde_json::to_vec(entry)?;
    if bytes.len() as u64 > MAX_ENTRY {
        return Ok(());
    }
    fs::create_dir_all(directory)?;
    check_directory(directory)?;
    let (compressed, legacy) = paths(directory, key);
    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
    let mut gzip = GzEncoder::new(temp.as_file_mut(), Compression::fast());
    gzip.write_all(&bytes)?;
    gzip.finish()?;

    let _guard = lock(directory)?;
    temp.persist(compressed).map_err(|err| err.error)?;
    if let Ok(meta) = fs::symlink_metadata(&legacy)
        && decode(&legacy, false, &meta).is_some_and(|previous| {
            previous.url == entry.url && previous.fetched_at <= entry.fetched_at
        })
    {
        let _ = remove_unchanged(&legacy, &meta);
    }
    // Leave headroom so a burst of downloads does not trigger a full sweep per entry.
    if let Ok(files) = candidates(directory)
        && files.iter().map(|file| file.meta.len()).sum::<u64>() > limit
    {
        sweep(files, limit, limit - limit / 8, true);
    }
    Ok(())
}

/// Preview or apply eviction of the oldest verified raw HTTP responses.
pub fn prune_cache(directory: &Path, max_mib: u32, apply: bool) -> Result<CachePruneReport> {
    // A trailing slash can make symlink_metadata follow the final symlink.
    let directory: PathBuf = directory.components().collect();
    let limit = u64::from(max_mib) * MIB;
    let exists = check_directory(&directory)?;
    let _guard = if exists && apply {
        Some(lock(&directory)?)
    } else {
        None
    };
    let files = if exists {
        candidates(&directory)?
    } else {
        Vec::new()
    };
    Ok(sweep(files, limit, limit, apply))
}

struct Candidate {
    path: PathBuf,
    compressed: bool,
    meta: Metadata,
}

fn candidates(directory: &Path) -> io::Result<Vec<Candidate>> {
    let mut files = Vec::new();
    for child in fs::read_dir(directory)? {
        let child = child?;
        let name = child.file_name();
        let Some(name) = name.to_str() else { continue };
        let (key, compressed) = if let Some(key) = name.strip_suffix(".json.gz") {
            (key, true)
        } else if let Some(key) = name.strip_suffix(".json") {
            (key, false)
        } else {
            continue;
        };
        if key.len() != 64 || !key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            continue;
        }
        let path = child.path();
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !is_redirected(&meta) => {
                files.push(Candidate {
                    path,
                    compressed,
                    meta,
                });
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(files)
}

fn sweep(files: Vec<Candidate>, limit: u64, target: u64, apply: bool) -> CachePruneReport {
    let mut report = CachePruneReport {
        apply,
        limit_bytes: limit,
        cache_files: 0,
        cache_bytes: 0,
        planned_files: 0,
        planned_bytes: 0,
        removed_files: 0,
        removed_bytes: 0,
        skipped_candidates: 0,
        errors: Vec::new(),
    };
    let mut verified = Vec::new();
    for file in files {
        if let Some(entry) = decode(&file.path, file.compressed, &file.meta) {
            report.cache_files += 1;
            report.cache_bytes += file.meta.len();
            verified.push((entry.fetched_at, file));
        } else {
            report.skipped_candidates += 1;
        }
    }
    if report.cache_bytes <= limit {
        return report;
    }
    verified.sort_unstable_by(|(date_a, a), (date_b, b)| {
        date_a.cmp(date_b).then_with(|| a.path.cmp(&b.path))
    });
    for (_, file) in verified {
        if report.cache_bytes - report.planned_bytes <= target {
            break;
        }
        report.planned_files += 1;
        report.planned_bytes += file.meta.len();
        if apply {
            match remove_unchanged(&file.path, &file.meta) {
                Ok(()) => {
                    report.removed_files += 1;
                    report.removed_bytes += file.meta.len();
                }
                Err(err) => report
                    .errors
                    .push(format!("{}: {err}", file.path.display())),
            }
        }
    }
    report
}

fn remove_unchanged(path: &Path, expected: &Metadata) -> io::Result<()> {
    let current = fs::symlink_metadata(path)?;
    if !current.is_file()
        || is_redirected(&current)
        || current.len() != expected.len()
        || current.modified()? != expected.modified()?
    {
        return Err(io::Error::other("cache file changed; left in place"));
    }
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(day: u32) -> Entry {
        Entry {
            url: "https://example.com/releases".into(),
            effective_url: None,
            fetched_at: DateTime::parse_from_rfc3339(&format!("2026-01-{day:02}T00:00:00Z"))
                .unwrap()
                .with_timezone(&Utc),
            headers: BTreeMap::from([("etag".into(), "notes".into())]),
            body: "release notes\n".repeat(100),
        }
    }

    fn legacy(path: &Path, entry: &Entry) {
        fs::write(path, serde_json::to_vec(entry).unwrap()).unwrap();
    }

    #[test]
    fn reads_legacy_and_replaces_it_with_smaller_gzip() {
        let dir = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        let (compressed, old) = paths(dir.path(), &key);
        let value = entry(1);
        legacy(&old, &value);
        let old_size = fs::metadata(&old).unwrap().len();
        assert_eq!(load(dir.path(), &key, &value.url).unwrap().body, value.body);
        assert!(!compressed.exists());
        store(dir.path(), &key, &value).unwrap();
        assert!(!old.exists());
        assert!(fs::metadata(&compressed).unwrap().len() < old_size);
        assert_eq!(
            load(dir.path(), &key, &value.url).unwrap().headers,
            value.headers
        );

        let newer = entry(2);
        legacy(&old, &newer);
        assert_eq!(
            load(dir.path(), &key, &value.url).unwrap().fetched_at,
            newer.fetched_at
        );
        store(dir.path(), &key, &value).unwrap();
        assert!(old.exists());
        fs::write(&compressed, "not gzip").unwrap();
        assert_eq!(
            load(dir.path(), &key, &value.url).unwrap().fetched_at,
            newer.fetched_at
        );
    }

    #[test]
    fn automatic_budget_keeps_the_newest_response() {
        let dir = tempfile::tempdir().unwrap();
        let old_key = "a".repeat(64);
        let new_key = "b".repeat(64);
        let (new_path, _) = paths(dir.path(), &new_key);
        let (_, old_path) = paths(dir.path(), &old_key);
        store(dir.path(), &new_key, &entry(2)).unwrap();
        let size = fs::metadata(&new_path).unwrap().len();
        legacy(&old_path, &entry(1));
        store_with_limit(dir.path(), &new_key, &entry(2), size * 8 / 7 + 2).unwrap();
        assert!(!old_path.exists());
        assert!(new_path.exists());
    }

    #[test]
    fn prune_preserves_unknown_files_and_only_applies_on_request() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let (_, raw) = paths(path, &"a".repeat(64));
        legacy(&raw, &entry(1));
        let note = path.join(format!("{}.json", "b".repeat(64)));
        let marker = path.join(format!("rate-{}.json", "c".repeat(64)));
        let nested = path.join("notes").join(format!("{}.json", "d".repeat(64)));
        fs::write(&note, r#"{"release_notes":["Keep this"]}"#).unwrap();
        fs::write(&marker, "[1769904000,429]").unwrap();
        fs::create_dir(nested.parent().unwrap()).unwrap();
        legacy(&nested, &entry(1));
        let preview = prune_cache(path, 0, false).unwrap();
        assert_eq!(preview.cache_files, 1);
        assert_eq!(preview.planned_files, 1);
        assert_eq!(preview.removed_files, 0);
        assert!(raw.exists());
        assert!(!path.join(".http-cache.lock").exists());
        let result = prune_cache(path, 0, true).unwrap();
        assert_eq!(result.removed_bytes, preview.planned_bytes);
        assert_eq!(result.skipped_candidates, 1);
        assert!(!raw.exists());
        assert!(note.exists() && marker.exists() && nested.exists());

        let absent = path.join("absent");
        assert_eq!(prune_cache(&absent, 0, true).unwrap().planned_files, 0);
        assert!(!absent.exists());
    }

    #[test]
    fn oversized_gzip_is_not_decoded_as_a_response() {
        let dir = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        let (path, _) = paths(dir.path(), &key);
        let mut gzip = GzEncoder::new(File::create(&path).unwrap(), Compression::fast());
        io::copy(&mut io::repeat(b'x').take(MAX_ENTRY + 1), &mut gzip).unwrap();
        gzip.finish().unwrap();
        assert!(load(dir.path(), &key, &entry(1).url).is_none());
        assert_eq!(
            prune_cache(dir.path(), 0, true).unwrap().skipped_candidates,
            1
        );
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn pruning_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let alias = dir.path().join("alias");
        let outside = dir.path().join("keep.json");
        fs::create_dir(&cache).unwrap();
        legacy(&outside, &entry(1));
        let (_, child) = paths(&cache, &"a".repeat(64));
        symlink(&outside, &child).unwrap();
        symlink(&cache, &alias).unwrap();
        assert_eq!(prune_cache(&cache, 0, true).unwrap().cache_files, 0);
        assert!(prune_cache(&alias, 0, true).is_err());
        assert!(prune_cache(&alias.join("."), 0, true).is_err());
        assert!(outside.exists() && child.exists());
    }
}
