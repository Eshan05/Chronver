use crate::ai_summary::Provider;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::LazyLock,
};

const DEFAULTS: &str = include_str!("../assets/ai-models.json");
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    schema_version: u8,
    providers: BTreeMap<String, Vec<Model>>,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_graphic())
}

fn parse(bytes: &[u8]) -> io::Result<Catalog> {
    let catalog: Catalog = serde_json::from_slice(bytes)?;
    let valid = catalog.schema_version == 1
        && catalog.providers.iter().all(|(key, models)| {
            let mut ids = BTreeSet::new();
            Provider::ALL.iter().any(|p| p.id() == key)
                && models.len() <= 64
                && models.iter().all(|m| {
                    valid_id(&m.id)
                        && ids.insert(m.id.as_str())
                        && !m.label.trim().is_empty()
                        && m.label.chars().count() <= 80
                        && !m.label.chars().any(char::is_control)
                })
        });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid model catalog",
        ));
    }
    Ok(catalog)
}

pub fn bundled() -> &'static Catalog {
    static BUNDLED: LazyLock<Catalog> =
        LazyLock::new(|| parse(DEFAULTS.as_bytes()).expect("valid bundled AI model catalog"));
    &BUNDLED
}

impl Catalog {
    pub fn models(&self, provider: Provider) -> &[Model] {
        self.providers.get(provider.id()).map_or(&[], Vec::as_slice)
    }

    pub fn default_model(&self, provider: Provider) -> &str {
        self.models(provider).first().map_or("", |m| m.id.as_str())
    }
}

pub fn load(path: &Path) -> io::Result<Catalog> {
    let mut catalog = bundled().clone();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(catalog),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "model catalog is too large",
        ));
    }
    catalog.providers.extend(parse(&bytes)?.providers);
    Ok(catalog)
}

pub fn path() -> PathBuf {
    crate::live::cache_directory().with_file_name("ai-models.json")
}

#[cfg(windows)]
pub fn edit_file() -> io::Result<()> {
    use std::{fs, io::Write};
    let path = path();
    let editor =
        PathBuf::from(std::env::var_os("SystemRoot").ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Windows directory not found")
        })?)
        .join("System32")
        .join("notepad.exe");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match File::options().write(true).create_new(true).open(&path) {
        Ok(mut file) => file.write_all(DEFAULTS.as_bytes())?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    std::process::Command::new(editor).arg(path).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_model_catalog_reloads_overrides_without_changing_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ai-models.json");
        assert_eq!(
            load(&path).unwrap().default_model(Provider::OpenAi),
            "gpt-6-luna"
        );
        std::fs::write(&path, r#"{"schema_version":1,"providers":{"openai":[{"id":"model-v2","label":"Model V2"}],"custom":[{"id":"vendor/model","label":"My gateway"}]}}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.default_model(Provider::OpenAi), "model-v2");
        assert_eq!(loaded.default_model(Provider::Claude), "claude-haiku-5-5");
        assert_eq!(loaded.default_model(Provider::Custom), "vendor/model");
        std::fs::write(&path, r#"{"schema_version":1,"providers":{"openai":[]}}"#).unwrap();
        assert!(load(&path).unwrap().models(Provider::OpenAi).is_empty());
    }

    #[test]
    fn ai_model_catalog_rejects_unknown_fields_bad_ids_and_duplicate_entries() {
        for json in [
            r#"{"schema_version":2,"providers":{}}"#,
            r#"{"schema_version":1,"providers":{"unknown":[]}}"#,
            r#"{"schema_version":1,"providers":{"openai":[{"id":"bad id","label":"Bad ID"}]}}"#,
            r#"{"schema_version":1,"providers":{"openai":[{"id":"a","label":"A"},{"id":"a","label":"B"}]}}"#,
            r#"{"schema_version":1,"providers":{"openai":[{"id":"a","label":"A","endpoint":"https://example.test"}]}}"#,
        ] {
            assert!(parse(json.as_bytes()).is_err());
        }
    }
}
