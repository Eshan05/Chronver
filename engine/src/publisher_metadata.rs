use crate::{http::Fetcher, icons::safe_url, source_discovery};
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

#[derive(Clone, Debug)]
pub enum Source {
    Website(Url),
    Github { owner: String, repo: String },
    Npm(String),
}

pub struct Metadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: String,
}

impl Source {
    pub fn github_hint(raw: &str) -> Option<Self> {
        let (owner, repo, _) = source_discovery::github(raw)?;
        (!repo.is_empty()).then_some(Self::Github { owner, repo })
    }

    pub fn website(raw: &str) -> Option<Self> {
        let mut url = safe_url(raw)?;
        url.set_query(None);
        url.set_fragment(None);
        Some(Self::Website(url))
    }

    pub fn fetch(&self, client: &impl Fetcher) -> Option<Metadata> {
        match self {
            Self::Website(url) => {
                let doc = client.get(url.as_str()).ok()?;
                let html = Html::parse_document(&doc.body);
                let value = |selector: &str| {
                    let selector = Selector::parse(selector).ok()?;
                    html.select(&selector)
                        .find_map(|e| clean(e.value().attr("content")?))
                };
                Some(Metadata {
                    name: value("meta[name='application-name']")
                        .or_else(|| value("meta[property='og:site_name']")),
                    description: value("meta[name='description']")
                        .or_else(|| value("meta[property='og:description']")),
                    url: self.public_url(),
                })
            }
            Self::Github { owner, repo } => {
                let url = format!("https://api.github.com/repos/{owner}/{repo}");
                let doc = client.get(&url).ok()?;
                let data: Value = serde_json::from_str(&doc.body).ok()?;
                if !data["full_name"]
                    .as_str()?
                    .eq_ignore_ascii_case(&format!("{owner}/{repo}"))
                {
                    return None;
                }
                Some(Metadata {
                    name: data["name"].as_str().and_then(clean),
                    description: data["description"].as_str().and_then(clean),
                    url: self.public_url(),
                })
            }
            Self::Npm(name) => {
                if !crate::dependencies::package_name(name) {
                    return None;
                }
                let mut url = Url::parse("https://registry.npmjs.org/").ok()?;
                url.path_segments_mut()
                    .ok()?
                    .pop_if_empty()
                    .push(name)
                    .push("latest");
                let doc = client.get(url.as_str()).ok()?;
                let data: Value = serde_json::from_str(&doc.body).ok()?;
                if data["name"] != name.as_str() {
                    return None;
                }
                Some(Metadata {
                    name: Some(name.clone()),
                    description: data["description"].as_str().and_then(clean),
                    url: self.public_url(),
                })
            }
        }
    }

    pub fn public_url(&self) -> String {
        match self {
            Self::Website(url) => url.to_string(),
            Self::Github { owner, repo } => format!("https://github.com/{owner}/{repo}"),
            Self::Npm(name) => format!("https://www.npmjs.com/package/{name}"),
        }
    }

    pub fn matches_citation(&self, raw: &str) -> bool {
        let Some(url) = safe_url(raw) else {
            return false;
        };
        match self {
            Self::Website(known) => {
                let host = |u: &Url| {
                    u.host_str()
                        .unwrap_or_default()
                        .trim_start_matches("www.")
                        .to_owned()
                };
                let known_host = host(known);
                if matches!(
                    known_host.as_str(),
                    "github.com" | "npmjs.com" | "registry.npmjs.org"
                ) {
                    return false;
                }
                known_host == host(&url)
                    && (known.path() == "/"
                        || url.path() == known.path()
                        || url
                            .path()
                            .strip_prefix(known.path().trim_end_matches('/'))
                            .is_some_and(|s| s.starts_with('/')))
            }
            Self::Github { owner, repo } => {
                source_discovery::github(raw).is_some_and(|(o, r, _)| {
                    o.eq_ignore_ascii_case(owner) && r.eq_ignore_ascii_case(repo)
                })
            }
            Self::Npm(name) => {
                matches!(url.host_str(), Some("www.npmjs.com" | "npmjs.com"))
                    && url
                        .path()
                        .strip_prefix(&format!("/package/{name}"))
                        .is_some_and(|path| path.is_empty() || path.starts_with('/'))
            }
        }
    }
}

fn clean(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty() && value.chars().count() <= 4000).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Evidence, Result, http::Document};

    struct Fixture(&'static str, &'static str);
    impl Fetcher for Fixture {
        fn get(&self, url: &str) -> Result<Document> {
            assert_eq!(url, self.0);
            Ok(Document {
                body: self.1.into(),
                headers: Default::default(),
                evidence: Evidence {
                    url: url.into(),
                    fetched_at: String::new(),
                    cache: String::new(),
                    content_sha256: None,
                },
            })
        }
    }

    #[test]
    fn publisher_metadata_uses_named_public_fields_not_generic_page_titles() {
        let source = Source::website("https://example.org/tool?private=token#part").unwrap();
        let doc = Fixture(
            "https://example.org/tool",
            "<title>Home</title><meta name='application-name' content='The Tool'><meta name='description' content='A tool for &amp; by users.'>",
        );
        let metadata = source.fetch(&doc).unwrap();
        assert_eq!(metadata.name.as_deref(), Some("The Tool"));
        assert_eq!(
            metadata.description.as_deref(),
            Some("A tool for & by users.")
        );
        assert!(!metadata.url.contains("private"));
        assert!(Source::website("http://127.0.0.1/").is_none());
        assert!(!source.matches_citation("https://example.org/other-project"));
    }

    #[test]
    fn publisher_metadata_rejects_wrong_repository_and_package() {
        let github = Source::github_hint("https://github.com/acme/tool/releases").unwrap();
        assert!(
            github
                .fetch(&Fixture(
                    "https://api.github.com/repos/acme/tool",
                    r#"{"full_name":"other/tool","name":"Tool"}"#
                ))
                .is_none()
        );
        assert!(github.matches_citation("https://github.com/acme/tool/releases"));
        assert!(!github.matches_citation("https://github.com/acme/another"));
        assert!(
            !Source::website("https://github.com/")
                .unwrap()
                .matches_citation("https://github.com/acme/another")
        );
        let npm = Source::Npm("the-tool".into());
        assert!(
            npm.fetch(&Fixture(
                "https://registry.npmjs.org/the-tool/latest",
                r#"{"name":"another"}"#
            ))
            .is_none()
        );
        assert!(npm.matches_citation("https://www.npmjs.com/package/the-tool/v/1.0.0"));
        assert!(!npm.matches_citation("https://www.npmjs.com/package/the-tool-malicious"));
    }
}
