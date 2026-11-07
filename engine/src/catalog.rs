use crate::{http::https, model::*, parse};
use regex::Regex;
use serde::Deserialize;
use std::{collections::HashSet, fs::File, io::Read, path::Path};

pub fn bundled() -> Result<Catalog> {
    let mut catalog: Catalog = serde_json::from_str(include_str!("../data/catalog.json"))?;
    catalog
        .packages
        .extend(load(include_str!("../data/catalog-supplement.json"))?.packages);
    validate(catalog)
}
pub fn load(json: &str) -> Result<Catalog> {
    validate(serde_json::from_str(json)?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceOverrides {
    schema_version: u32,
    products: Vec<ProductOverride>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductOverride {
    id: String,
    #[serde(default)]
    reference_urls: Vec<String>,
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrackingPreferences {
    schema_version: u32,
    products: Vec<TrackingPreference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrackingPreference {
    id: String,
    tracking: String,
}

fn read_settings<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    const LIMIT: u64 = 256 * 1024;
    let mut bytes = Vec::new();
    File::open(path)?.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Config("local settings exceed 256 KiB".into()));
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    Ok(serde_json::from_slice(bytes)?)
}

/// Add source hints or replace individual source profiles for known product IDs.
/// If any override is invalid, the existing catalog is left unchanged.
pub fn apply_source_overrides(catalog: &mut Catalog, path: &Path) -> Result<()> {
    let config: SourceOverrides = read_settings(path)?;
    if config.schema_version != 1 {
        return Err(Error::Config("unsupported source override schema".into()));
    }
    let mut next = catalog.clone();
    let mut products = HashSet::new();
    for entry in config.products {
        if !products.insert(entry.id.clone())
            || (entry.streams.is_empty() && entry.reference_urls.is_empty())
        {
            return Err(Error::Config(format!(
                "duplicate product or empty source override for {}", entry.id
            )));
        }
        let product = next.packages.iter_mut().find(|p| p.id == entry.id)
            .ok_or_else(|| Error::Config(format!("unknown source override product {}", entry.id)))?;
        if !entry.reference_urls.is_empty() {
            let mut seen = HashSet::new();
            product.reference_urls = entry.reference_urls.into_iter()
                .chain(std::mem::take(&mut product.reference_urls))
                .filter(|url| seen.insert(url.clone()))
                .collect();
        }
        let mut streams = HashSet::new();
        for stream in entry.streams {
            if !streams.insert(stream.id.clone())
                || stream.sources.iter().any(|s| matches!(s, Source::InstalledDocument { .. }))
            {
                return Err(Error::Config(format!(
                    "duplicate stream ID or local document in source override for {}", entry.id
                )));
            }
            if let Some(existing) = product.streams.iter_mut().find(|s| s.id == stream.id) {
                if existing.basis != stream.basis {
                    return Err(Error::Config(format!(
                        "source override for {}/{} changes its query basis; use a new stream ID",
                        entry.id, stream.id
                    )));
                }
                *existing = stream;
            } else {
                product.streams.push(stream);
            }
        }
    }
    *catalog = validate(next)?;
    Ok(())
}

/// Apply user tracking choices without changing matching or source profiles.
pub fn apply_tracking_preferences(catalog: &mut Catalog, path: &Path) -> Result<()> {
    let config: TrackingPreferences = read_settings(path)?;
    if config.schema_version != 1 {
        return Err(Error::Config("unsupported tracking preferences schema".into()));
    }
    let mut seen = HashSet::new();
    let mut changes = Vec::new();
    for entry in config.products {
        if !seen.insert(entry.id.clone())
            || !matches!(entry.tracking.as_str(), "track" | "low_priority" | "deferred" | "excluded")
        {
            return Err(Error::Config(format!(
                "duplicate product or invalid tracking preference for {}", entry.id
            )));
        }
        let index = catalog.packages.iter().position(|p| p.id == entry.id)
            .ok_or_else(|| Error::Config(format!("unknown tracking product {}", entry.id)))?;
        changes.push((index, entry.tracking));
    }
    for (index, tracking) in changes {
        catalog.packages[index].tracking = tracking;
    }
    Ok(())
}

fn validate(catalog: Catalog) -> Result<Catalog> {
    if catalog.schema_version != 1 {
        return Err(Error::Config("unsupported catalog schema".into()));
    }
    let mut ids = HashSet::new();
    for p in &catalog.packages {
        if p.id.is_empty() || !ids.insert(&p.id) {
            return Err(Error::Config("empty/duplicate package ID".into()));
        }
        if ![
            "track",
            "low_priority",
            "deferred",
            "excluded",
            "covered_elsewhere",
            "reference_only",
        ]
        .contains(&p.tracking.as_str())
        {
            return Err(Error::Config(format!(
                "unknown tracking preference for {}",
                p.id
            )));
        }
        for url in &p.reference_urls {
            https(url)?;
        }
        let mut streams = HashSet::new();
        for stream in &p.streams {
            if stream.id.is_empty() || !streams.insert(&stream.id) || stream.sources.is_empty() {
                return Err(Error::Config(format!("invalid streams for {}", p.id)));
            }
            if stream.npm_date_package.as_deref().is_some_and(|name| {
                stream.basis != Basis::Version || !crate::dependencies::package_name(name)
            }) {
                return Err(Error::Config(format!("invalid npm date package for {}/{}", p.id, stream.id)));
            }
            for source in &stream.sources {
                match source {
                    Source::InstalledDocument {
                        root,
                        filename,
                        package,
                        version,
                    } => {
                        if !std::path::Path::new(root).is_absolute()
                            || !["CHANGELOG.md", "changelog.md", "CHANGES.md", "NEWS.md"]
                                .contains(&filename.as_str())
                            || package.is_empty()
                            || version.is_empty()
                            || stream.basis != Basis::Version
                        {
                            return Err(Error::Config(
                                "invalid installed changelog profile".into(),
                            ));
                        }
                    }
                    Source::Github {
                        owner,
                        repo,
                        max_pages,
                        tag_pattern,
                        tag_prefix,
                        ..
                    } => {
                        let valid = |s: &str| {
                            !s.is_empty()
                                && !matches!(s, "." | "..")
                                && s.chars()
                                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                        };
                        if !valid(owner)
                            || !valid(repo)
                            || !(1..=10).contains(max_pages)
                            || stream.basis != Basis::Version
                        {
                            return Err(Error::Config("invalid GitHub source/budget".into()));
                        }
                        if let Some(pattern) = tag_pattern {
                            let re =
                                Regex::new(pattern).map_err(|e| Error::Config(e.to_string()))?;
                            if !tag_prefix.is_empty()
                                || !re.capture_names().flatten().any(|n| n == "version")
                            {
                                return Err(Error::Config(
                                    "tag_pattern needs version capture and an empty tag_prefix"
                                        .into(),
                                ));
                            }
                        }
                    }
                    Source::Document {
                        url,
                        boundary,
                        selector,
                        format,
                        paragraphs,
                        exclude,
                        ..
                    } => {
                        https(url)?;
                        if let Some(css) = exclude {
                            if !matches!(format, Format::Html | Format::HtmlEntries | Format::Plain)
                            {
                                return Err(Error::Config(
                                    "HTML exclusions require HTML content".into(),
                                ));
                            }
                            parse::selector(css)?;
                        }
                        let re = Regex::new(boundary).map_err(|e| Error::Config(e.to_string()))?;
                        let capture = if stream.basis == Basis::Version {
                            "version"
                        } else {
                            "date"
                        };
                        if !re.capture_names().flatten().any(|n| n == capture) {
                            return Err(Error::Config(format!("missing {capture} boundary")));
                        }
                        if let Some(css) = selector {
                            if matches!(format, Format::JsonMarkdown) {
                                if (!css.is_empty() && !css.starts_with('/'))
                                    || css.matches("/*").count() > 1
                                    || css.len() > 512
                                {
                                    return Err(Error::Config(
                                        "invalid JSON Markdown pointer".into(),
                                    ));
                                }
                            } else {
                                parse::selector(css)?;
                            }
                        }
                        if matches!(format, Format::Plain) && *paragraphs {
                            return Err(Error::Config(
                                "plain documents cannot use paragraph mode".into(),
                            ));
                        }
                        if matches!(format, Format::HtmlEntries | Format::JsonMarkdown)
                            && selector.is_none()
                        {
                            return Err(Error::Config(
                                "HTML entries require an explicit entry selector".into(),
                            ));
                        }
                    }
                    Source::Feed { url } => {
                        https(url)?;
                        if stream.basis != Basis::Date {
                            return Err(Error::Config("feeds require date basis".into()));
                        }
                    }
                    Source::LinkedDocuments {
                        url,
                        links,
                        content,
                        version_pattern,
                        boundary,
                        allowed_origins,
                        max_documents,
                    } => {
                        https(url)?;
                        parse::selector(links)?;
                        parse::selector(content)?;
                        for origin in allowed_origins {
                            let parsed = https(origin)?;
                            if parsed.origin().ascii_serialization() != *origin {
                                return Err(Error::Config(
                                    "linked-document allowance must be an HTTPS origin".into(),
                                ));
                            }
                        }
                        if let Some(boundary) = boundary {
                            let re =
                                Regex::new(boundary).map_err(|e| Error::Config(e.to_string()))?;
                            if !re.capture_names().flatten().any(|n| n == "version") {
                                return Err(Error::Config(
                                    "linked documents need version boundaries".into(),
                                ));
                            }
                        }
                        let re = Regex::new(version_pattern)
                            .map_err(|e| Error::Config(e.to_string()))?;
                        if !re.capture_names().flatten().any(|n| n == "version")
                            || !(1..=50).contains(max_documents)
                            || stream.basis != Basis::Version
                        {
                            return Err(Error::Config("invalid linked-document profile".into()));
                        }
                    }
                }
            }
        }
    }
    for p in &catalog.packages {
        if let Some(alias) = &p.alias_of
            && (alias == &p.id || !ids.contains(alias))
        {
            return Err(Error::Config(format!("invalid alias for {}", p.id)));
        }
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(value: serde_json::Value) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), value.to_string()).unwrap();
        file
    }

    #[test]
    fn public_catalog_does_not_ship_personal_tracking_choices() {
        let catalog = bundled().unwrap();
        assert!(catalog.packages.iter().all(|p| {
            matches!(p.tracking.as_str(), "track" | "covered_elsewhere")
        }));
        crate::matching::validate(&catalog).unwrap();
    }

    #[test]
    fn source_overrides_are_atomic_and_preserve_other_components() {
        let mut catalog = bundled().unwrap();
        let package = |catalog: &Catalog| {
            serde_json::to_value(catalog.packages.iter().find(|p| p.id == "podman").unwrap()).unwrap()
        };
        let before = package(&catalog);
        let mut stream = before["streams"][0].clone();
        stream["scope"] = serde_json::json!("Local engine source rule");
        let entry = serde_json::json!({"id":"podman","streams":[stream]});
        let bad = config(serde_json::json!({"schema_version":1,"products":[entry,{"id":"unknown","streams":[stream]}]}));
        assert!(apply_source_overrides(&mut catalog, bad.path()).is_err());
        assert_eq!(package(&catalog), before);
        let good = config(serde_json::json!({"schema_version":1,"products":[entry]}));
        apply_source_overrides(&mut catalog, good.path()).unwrap();
        let after = package(&catalog);
        assert_eq!(after["streams"][0]["scope"], "Local engine source rule");
        assert_eq!(before["streams"][1], after["streams"][1]);
        assert_eq!(before["tracking"], after["tracking"]);
        crate::matching::validate(&catalog).unwrap();
    }

    #[test]
    fn local_settings_validate_sources_and_tracking_without_partial_application() {
        let mut catalog = bundled().unwrap();
        let before = serde_json::to_value(&catalog).unwrap();
        let bad_source = config(serde_json::json!({"schema_version":1,"products":[{
            "id":"gh", "streams":[{"id":"new","scope":"Invalid","basis":"version","sources":[
                {"kind":"document","url":"https://example.com/notes","format":"markdown","boundary":"missing capture"}
            ]}]
        }]}));
        assert!(apply_source_overrides(&mut catalog, bad_source.path()).is_err());
        let bad_preferences = config(serde_json::json!({"schema_version":1,"products":[
            {"id":"gh","tracking":"excluded"},{"id":"gh","tracking":"track"}
        ]}));
        assert!(apply_tracking_preferences(&mut catalog, bad_preferences.path()).is_err());
        assert_eq!(serde_json::to_value(&catalog).unwrap(), before);
        let preferences = config(serde_json::json!({"schema_version":1,"products":[
            {"id":"gh","tracking":"excluded"}
        ]}));
        apply_tracking_preferences(&mut catalog, preferences.path()).unwrap();
        let gh = catalog.packages.iter().find(|p| p.id == "gh").unwrap();
        assert_eq!(gh.tracking, "excluded");
        assert!(!gh.streams.is_empty());
    }

    #[test]
    fn source_hint_overlay_prepends_urls_without_inventing_a_profile() {
        let mut catalog = bundled().unwrap();
        let original = catalog.packages.iter().find(|p| p.id == "vsc").unwrap().reference_urls.clone();
        let preferred = "https://example.com/insiders/notes";
        let hints = config(serde_json::json!({"schema_version":1,"products":[
            {"id":"vsc","reference_urls":[preferred,preferred]}
        ]}));
        apply_source_overrides(&mut catalog, hints.path()).unwrap();
        let vsc = catalog.packages.iter().find(|p| p.id == "vsc").unwrap();
        assert_eq!(vsc.reference_urls[0], preferred);
        assert_eq!(&vsc.reference_urls[1..], original.as_slice());
        assert!(vsc.streams.is_empty());
    }

    #[test]
    fn catalog_preserves_reference_inventory() {
        let catalog = super::bundled().unwrap();
        assert!(catalog.packages.len() >= 139);
        let nightly = &catalog
            .packages
            .iter()
            .find(|p| p.id == "t3-code")
            .unwrap()
            .streams[0];
        assert_eq!(nightly.channel, crate::Channel::Preview);
        if let crate::Source::Github {
            tag_pattern: Some(pattern),
            ..
        } = &nightly.sources[0]
        {
            let re = regex::Regex::new(&format!("^(?:{pattern})$")).unwrap();
            assert!(re.is_match("v0.1.9-nightly.20261110.4452"));
            assert!(!re.is_match("v0.1.8-preview.20261109.4440"));
            assert!(!re.is_match("v0.1.8"));
        } else {
            panic!("nightly-only tag profile missing");
        }
        assert_eq!(
            catalog
                .packages
                .iter()
                .find(|p| p.id == "supabase")
                .unwrap()
                .streams
                .len(),
            2
        );
        assert!(
            catalog
                .packages
                .iter()
                .find(|p| p.id == "vsc")
                .unwrap()
                .streams
                .is_empty()
        );
        assert_eq!(
            catalog
                .packages
                .iter()
                .find(|p| p.id == "atlas-cli")
                .unwrap()
                .identity,
            "confirmed"
        );
    }
}
