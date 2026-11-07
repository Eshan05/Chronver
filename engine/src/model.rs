use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("invalid query: {0}")]
    Query(String),
    #[error("unsupported version: {0}")]
    Version(String),
    #[error("source failed: {0}")]
    Source(String),
    #[error("HTTP {status} for {url}{detail}")]
    Http {
        status: u16,
        url: String,
        detail: String,
    },
    #[error("offline cache miss: {0}")]
    Offline(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Network(#[from] reqwest::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    #[default]
    Semver,
    Numeric,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    #[default]
    Stable,
    Preview,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Version,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub packages: Vec<Package>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub tracking: String,
    pub identity: String,
    #[serde(default)]
    pub alias_of: Option<String>,
    #[serde(default)]
    pub caveats: String,
    #[serde(default)]
    pub reference_urls: Vec<String>,
    #[serde(default)]
    pub streams: Vec<Stream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stream {
    pub id: String,
    pub scope: String,
    pub basis: Basis,
    #[serde(default)]
    pub scheme: Scheme,
    #[serde(default)]
    pub channel: Channel,
    #[serde(default)]
    pub caveats: String,
    /// Exact npm package identity used only to date otherwise undated notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_date_package: Option<String>,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    InstalledDocument {
        root: String,
        filename: String,
        package: String,
        version: String,
    },
    Github {
        owner: String,
        repo: String,
        #[serde(default)]
        tag_prefix: String,
        /// Optional whole-tag matcher with a named `version` capture; filters
        /// unrelated monorepo releases and supports historical tag spellings.
        #[serde(default)]
        tag_pattern: Option<String>,
        #[serde(default = "default_pages")]
        max_pages: usize,
    },
    Document {
        url: String,
        format: Format,
        /// Regex against a whole heading/paragraph, with a named `version` or `date` group.
        boundary: String,
        #[serde(default)]
        selector: Option<String>,
        /// Explicitly excluded HTML widgets/metadata, before content extraction.
        #[serde(default)]
        exclude: Option<String>,
        #[serde(default)]
        date_formats: Vec<String>,
        #[serde(default)]
        paragraphs: bool,
    },
    Feed {
        url: String,
    },
    /// Follow versioned release-note links from a bounded same-origin index.
    LinkedDocuments {
        url: String,
        links: String,
        version_pattern: String,
        content: String,
        /// Optional release boundaries when each linked page contains multiple releases.
        #[serde(default)]
        boundary: Option<String>,
        /// Explicit additional documentation origins, never inferred from arbitrary links.
        #[serde(default)]
        allowed_origins: Vec<String>,
        #[serde(default = "default_documents")]
        max_documents: usize,
    },
}
fn default_pages() -> usize {
    3
}
fn default_documents() -> usize {
    20
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Markdown,
    Html,
    HtmlEntries,
    /// JSON string(s) selected by a pointer, with one optional array wildcard.
    JsonMarkdown,
    Plain,
}

#[derive(Debug, Clone)]
pub enum Selection {
    Versions {
        from: String,
        to: String,
        channel: Channel,
    },
    Dates {
        since: NaiveDate,
        until: NaiveDate,
    },
    Inspect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub title: String,
    pub version: Option<String>,
    pub date: Option<NaiveDate>,
    pub url: String,
    pub body: String,
    pub authored_notes: bool,
    pub preview: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub url: String,
    pub fetched_at: String,
    pub cache: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub source: Source,
    pub outcome: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub package: String,
    pub stream: String,
    pub scope: String,
    pub basis: Basis,
    pub status: String,
    /// Completeness concerns retrieved authored notes, not every upstream software version.
    pub complete: bool,
    pub notes: Vec<Note>,
    pub warnings: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub attempts: Vec<Attempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryCursor {
    pub package: String,
    pub stream: String,
    pub source_index: usize,
    pub source_key: String,
    pub page: usize,
    pub offset: usize,
}
#[derive(Debug, Serialize)]
pub struct HistoryPage {
    pub report: Report,
    pub next: Option<HistoryCursor>,
}

pub(crate) struct Retrieved {
    pub notes: Vec<Note>,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub evidence: Vec<Evidence>,
}
