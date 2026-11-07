use releasescout_engine::inventory::{InstalledPackage, Manager};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selector {
    Catalog {
        id: String,
    },
    Package {
        manager: String,
        id: String,
    },
    Local {
        name: String,
        publisher: Option<String>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub product: String,
    pub description: String,
    pub sources: Vec<String>,
    pub selectors: Vec<Selector>,
}

fn folded(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn local_name(name: &str) -> String {
    let mut words: Vec<_> = name.split_whitespace().collect();
    if words.last().is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "(64-bit)" | "(32-bit)" | "(x64)" | "(x86)"
        )
    }) {
        words.pop();
    }
    let version = |text: &str| {
        let mut text = text.trim_matches(['(', ')']).trim_start_matches(['v', 'V']);
        for suffix in ["-x64", "-x86", "-arm64"] {
            text = text.strip_suffix(suffix).unwrap_or(text);
        }
        text.contains('.')
            && text
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    };
    if words.last().is_some_and(|s| version(s)) {
        words.pop();
        if words
            .last()
            .is_some_and(|s| s.eq_ignore_ascii_case("version"))
        {
            words.pop();
        }
    }
    folded(&words.join(" "))
}

fn manager_key(manager: &str) -> String {
    match manager {
        "npm" | "pnpm" | "bun" => "npm",
        other => other,
    }
    .into()
}

impl Selector {
    fn normalized(&self) -> Self {
        match self {
            Self::Catalog { id } => Self::Catalog { id: id.clone() },
            Self::Package { manager, id } => Self::Package {
                manager: manager_key(manager),
                id: id.to_ascii_lowercase(),
            },
            Self::Local { name, publisher } => Self::Local {
                name: local_name(name),
                publisher: publisher.as_deref().map(folded),
            },
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Catalog { id } => !id.is_empty() && !id.contains(['{', '}', '\\', '/']),
            Self::Package { manager, id } => {
                manager != "local"
                    && !id.is_empty()
                    && !id.contains(['{', '}', '\\'])
                    && Manager::ALL
                        .iter()
                        .any(|m| m.id() == manager && !m.is_editor())
            }
            Self::Local { name, publisher } => {
                !name.is_empty()
                    && !name.contains(['{', '}', '\\'])
                    && publisher.as_ref().is_none_or(|s| !s.is_empty())
            }
        }
    }
}

struct Catalog {
    entries: Vec<Entry>,
    index: BTreeMap<Selector, usize>,
}

impl Catalog {
    fn parse(json: &str) -> Result<Self, String> {
        let entries: Vec<Entry> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut index = BTreeMap::new();
        for (i, entry) in entries.iter().enumerate() {
            if entry.product.trim().is_empty()
                || entry.description.trim().is_empty()
                || entry.description.chars().count() > 160
                || entry.description.contains(['\n', '\r'])
                || entry.sources.is_empty()
                || entry.selectors.is_empty()
                || entry.sources.iter().any(|s| {
                    url::Url::parse(s).map_or(true, |u| {
                        !matches!(u.scheme(), "http" | "https") || u.host_str().is_none()
                    })
                })
            {
                return Err(format!("Invalid sourced description: {}", entry.product));
            }
            for selector in &entry.selectors {
                let selector = selector.normalized();
                if !selector.valid() {
                    return Err(format!("Invalid description selector: {selector:?}"));
                }
                if let Some(old) = index.insert(selector.clone(), i)
                    && old != i
                {
                    return Err(format!("Conflicting description selector: {selector:?}"));
                }
            }
        }
        Ok(Self { entries, index })
    }
    fn get(&self, selector: Selector) -> Option<&Entry> {
        self.index
            .get(&selector.normalized())
            .map(|&i| &self.entries[i])
    }
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    Catalog::parse(include_str!("../assets/software-descriptions.json"))
        .expect("valid sourced description catalog")
});

#[cfg(test)]
pub fn validate_catalog(json: &str) -> Result<(), String> {
    Catalog::parse(json).map(|_| ())
}

pub fn reference(id: &str) -> Option<&'static Entry> {
    CATALOG.get(Selector::Catalog { id: id.into() })
}

pub fn lookup(record: &InstalledPackage, catalog_id: Option<&str>) -> Option<&'static Entry> {
    CATALOG
        .get(Selector::Package {
            manager: record.manager.id().into(),
            id: record.id.clone(),
        })
        .or_else(|| catalog_id.and_then(reference))
        .or_else(|| {
            (record.manager == Manager::Local)
                .then(|| {
                    CATALOG.get(Selector::Local {
                        name: record.name.clone(),
                        publisher: record.evidence.publisher.clone(),
                    })
                })
                .flatten()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_version_normalization_is_conservative_and_keeps_product_editions() {
        for (raw, expected) in [
            ("Python 3.14.2 (64-bit)", "python"),
            ("PowerShell 7.6.10.0-x64", "powershell"),
            ("Malwarebytes version 5.4.1.215", "malwarebytes"),
            ("Pocket Tanks v1.6", "pocket tanks"),
            (
                "Office Home and Student 2019 - en-us",
                "office home and student 2019 - en-us",
            ),
            ("DirectX 12", "directx 12"),
            ("Python Launcher", "python launcher"),
        ] {
            assert_eq!(local_name(raw), expected);
        }
    }
    #[test]
    fn selectors_are_exact_portable_and_publisher_qualified() {
        let catalog = Catalog::parse(r#"[{"product":"Fixture","description":"A development tool.","sources":["https://example.com/about"],"selectors":[{"kind":"local","name":"Fixture","publisher":"Vendor"},{"kind":"package","manager":"npm","id":"@vendor/fixture"}]}]"#).unwrap();
        assert!(
            catalog
                .get(Selector::Local {
                    name: "Fixture 2.0 (64-bit)".into(),
                    publisher: Some("vendor".into())
                })
                .is_some()
        );
        assert!(
            catalog
                .get(Selector::Local {
                    name: "Fixture 2.0".into(),
                    publisher: Some("Different Vendor".into())
                })
                .is_none()
        );
        assert!(
            catalog
                .get(Selector::Local {
                    name: "Fixture Helper".into(),
                    publisher: Some("Vendor".into())
                })
                .is_none()
        );
        assert!(
            catalog
                .get(Selector::Package {
                    manager: "pnpm".into(),
                    id: "@vendor/fixture".into()
                })
                .is_some()
        );
        assert!(
            catalog
                .get(Selector::Package {
                    manager: "npm".into(),
                    id: "@other/fixture".into()
                })
                .is_none()
        );
        assert!(CATALOG.entries.iter().all(|e| !e.sources.is_empty()));
    }

    #[test]
    fn fallback_descriptions_never_displace_provider_text_or_personal_overrides() {
        static FALLBACK: LazyLock<Entry> = LazyLock::new(|| Entry {
            product: "Fixture".into(),
            description: "Reviewed fallback".into(),
            sources: vec!["https://example.com/about".into()],
            selectors: vec![],
        });
        let mut s = crate::model::State::demo();
        let row = crate::model::PackageRow {
            id: "fixture".into(),
            name: "Fixture".into(),
            package: 0,
            members: vec![0, 1],
        };
        s.catalog.packages[0].description = None;
        s.catalog.packages[0].description_fallback = Some(&FALLBACK);
        s.catalog.packages[1].description = None;
        s.catalog.packages[1].description_fallback = None;
        assert_eq!(
            crate::sidebar_info::description(&s, &row),
            Some("Reviewed fallback")
        );
        s.catalog.packages[1].description = Some("Provider text".into());
        assert_eq!(
            crate::sidebar_info::description(&s, &row),
            Some("Provider text")
        );
        s.activity.data.description_overrides.insert(
            crate::custom_icons::identity(&s.catalog.packages[0]),
            "My description".into(),
        );
        assert_eq!(
            crate::sidebar_info::description(&s, &row),
            Some("My description")
        );
        assert_eq!(
            s.catalog.packages[1].description.as_deref(),
            Some("Provider text")
        );
    }

    #[test]
    fn invalid_sources_machine_specific_selectors_and_conflicting_aliases_are_rejected() {
        let fixture = serde_json::json!({"product":"Fixture","description":"A tool.","sources":["https://example.com/about"],
            "selectors":[{"kind":"package","manager":"winget","id":"Vendor.Fixture"}]});
        let mut bad = fixture.clone();
        bad["sources"] = serde_json::json!([]);
        assert!(Catalog::parse(&serde_json::json!([bad]).to_string()).is_err());
        let mut bad = fixture.clone();
        bad["selectors"][0]["id"] = "ARP\\Machine\\X64\\{GUID}".into();
        assert!(Catalog::parse(&serde_json::json!([bad]).to_string()).is_err());
        let mut other = fixture.clone();
        other["product"] = "Different Product".into();
        assert!(Catalog::parse(&serde_json::json!([fixture, other]).to_string()).is_err());
    }
}
