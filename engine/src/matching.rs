//! Manager identities and publisher/site-scoped registrations bind reviewed profiles, never bare display names.
use crate::{
    Catalog, Error, Result,
    inventory::{InstalledPackage, Scan},
};
use serde::Deserialize;
use std::{io::Write, path::Path};

#[derive(Debug, Deserialize)]
pub struct Binding {
    pub manager: String,
    #[serde(default)]
    pub id: String,
    pub product: String,
    pub stream: Option<String>,
    pub source: Option<String>,
    pub publisher: Option<String>,
    pub name_pattern: Option<String>,
    pub homepage: Option<String>,
    pub id_pattern: Option<String>,
    pub display_name: Option<String>,
    #[serde(skip)]
    matcher: Option<regex::Regex>,
}
pub fn bindings() -> Vec<Binding> {
    let mut bindings: Vec<Binding> =
        serde_json::from_str(include_str!("../data/inventory-bindings.json"))
            .expect("bundled inventory bindings");
    for binding in &mut bindings {
        binding.matcher = binding
            .name_pattern
            .as_ref()
            .or(binding.id_pattern.as_ref())
            .map(|pattern| {
                regex::Regex::new(&format!("^(?:{pattern})$")).expect("bundled identity pattern")
            });
    }
    bindings
}
pub fn resolve<'a>(p: &InstalledPackage, bindings: &'a [Binding]) -> Option<&'a Binding> {
    let mut matches = bindings.iter().filter(|b| {
        b.manager == p.manager.id()
            && if b.name_pattern.is_some() {
                b.manager == "local"
                    && b.id.is_empty()
                    && (b.publisher.as_ref().is_some_and(|s| !s.trim().is_empty())
                        || b.homepage.is_some())
                    && b.publisher.as_ref().is_none_or(|publisher| {
                        p.evidence
                            .publisher
                            .as_ref()
                            .is_some_and(|v| v.eq_ignore_ascii_case(publisher))
                    })
                    && b.homepage.as_ref().is_none_or(|home| {
                        let key = |u: &str| {
                            url::Url::parse(u)
                                .ok()
                                .filter(|u| {
                                    matches!(u.scheme(), "https" | "http")
                                        && u.username().is_empty()
                                        && u.password().is_none()
                                })
                                .map(|u| {
                                    (
                                        u.host_str().unwrap_or("").to_ascii_lowercase(),
                                        u.path().trim_end_matches('/').to_string(),
                                        u.port(),
                                    )
                                })
                        };
                        key(home).is_some() && p.homepage.as_ref().and_then(|u| key(u)) == key(home)
                    })
                    && b.matcher.as_ref().is_some_and(|r| r.is_match(&p.name))
            } else if b.id_pattern.is_some() {
                b.manager != "local"
                    && b.id.is_empty()
                    && b.matcher.as_ref().is_some_and(|r| r.is_match(&p.id))
            } else {
                b.id == p.id
            }
            && b.source.as_ref().is_none_or(|source| source == &p.source)
    });
    let result = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(result)
    }
}
pub fn validate(catalog: &Catalog) -> Result<()> {
    let all = bindings();
    let mut seen = std::collections::HashSet::new();
    for b in &all {
        if !seen.insert((
            &b.manager,
            &b.id,
            &b.source,
            &b.publisher,
            &b.name_pattern,
            &b.homepage,
            &b.id_pattern,
        )) {
            return Err(Error::Config("duplicate inventory binding".into()));
        }
        if b.name_pattern.is_some() || b.publisher.is_some() || b.homepage.is_some() {
            if b.manager != "local"
                || !b.id.is_empty()
                || (b.publisher.as_ref().is_none_or(|s| s.trim().is_empty())
                    && b.homepage.is_none())
                || b.id_pattern.is_some()
            {
                return Err(Error::Config("registration bindings need a publisher and local name pattern, not an arbitrary ID".into()));
            }
            regex::Regex::new(b.name_pattern.as_deref().unwrap_or("(?!)"))
                .map_err(|e| Error::Config(e.to_string()))?;
        }
        if let Some(pattern) = &b.id_pattern {
            if !b.id.is_empty() || b.manager == "local" {
                return Err(Error::Config("invalid manager ID pattern".into()));
            }
            regex::Regex::new(pattern).map_err(|e| Error::Config(e.to_string()))?;
        }
        let p = catalog
            .packages
            .iter()
            .find(|p| p.id == b.product)
            .ok_or_else(|| Error::Config(format!("unknown binding product {}", b.product)))?;
        if let Some(stream) = &b.stream
            && !p.streams.iter().any(|s| &s.id == stream)
        {
            return Err(Error::Config(format!("unknown binding stream {stream}")));
        }
    }
    Ok(())
}
pub fn load_snapshot(path: &Path) -> Result<Vec<Scan>> {
    if std::fs::metadata(path)?.len() > 16 * 1024 * 1024 {
        return Err(Error::Config("inventory snapshot too large".into()));
    }
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn save_snapshot(path: &Path, scans: &[Scan]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Config("snapshot parent missing".into()))?;
    std::fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec(scans)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Error::Config("inventory snapshot too large".into()));
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(&bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
