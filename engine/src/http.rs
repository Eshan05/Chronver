pub use crate::http_cache::{CachePruneReport, DEFAULT_CACHE_MIB, prune_cache};
use crate::{
    http_cache::{self, Entry, MAX_BODY},
    model::{Error, Evidence, Result},
};
use chrono::{DateTime, Utc};
use reqwest::{blocking::Client, redirect::Policy};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Read, path::PathBuf, time::Duration};
use url::Url;

pub struct Request {
    pub url: Url,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}
pub trait Transport {
    fn send(&self, request: Request) -> Result<Response>;
}
pub struct Network {
    client: Client,
    public_only: bool,
}
impl Network {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .redirect(Policy::none())
                .user_agent(crate::USER_AGENT)
                .build()?,
            public_only: false,
        })
    }
}
impl Transport for Network {
    fn send(&self, request: Request) -> Result<Response> {
        let client = if self.public_only {
            use std::net::ToSocketAddrs;
            crate::icons::safe_url(request.url.as_str())
                .ok_or_else(|| Error::Source("unsafe discovered source URL".into()))?;
            let host = request.url.host_str().unwrap();
            let addresses: Vec<_> = (host, 443).to_socket_addrs()?.collect();
            if addresses.is_empty() || addresses.iter().any(|a| !crate::icons::public(a.ip())) {
                return Err(Error::Source(
                    "discovered source resolves to a non-public address".into(),
                ));
            }
            Client::builder()
                .no_proxy()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(12))
                .connect_timeout(Duration::from_secs(5))
                .user_agent(crate::USER_AGENT)
                .resolve_to_addrs(host, &addresses)
                .build()?
        } else {
            self.client.clone()
        };
        let mut builder = if let Some(body) = request.body {
            client.post(request.url).body(body)
        } else {
            client.get(request.url)
        };
        for (key, value) in request.headers {
            builder = builder.header(key, value);
        }
        let response = builder.send()?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_owned())))
            .collect();
        if response.content_length().is_some_and(|n| n > MAX_BODY) {
            return Err(Error::Source("response exceeds 8 MiB".into()));
        }
        let mut bytes = Vec::new();
        response.take(MAX_BODY + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BODY {
            return Err(Error::Source("response exceeds 8 MiB".into()));
        }
        let body =
            String::from_utf8(bytes).map_err(|_| Error::Source("response is not UTF-8".into()))?;
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

#[derive(Clone)]
pub struct CacheOptions {
    pub public_only: bool,
    pub directory: PathBuf,
    pub ttl: Duration,
    pub offline: bool,
    pub refresh: bool,
    pub github_token: Option<String>,
}
impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            public_only: false,
            directory: ".releasescout-cache".into(),
            ttl: Duration::from_secs(900),
            offline: false,
            refresh: false,
            github_token: None,
        }
    }
}
#[derive(Debug)]
pub struct Document {
    pub body: String,
    pub headers: BTreeMap<String, String>,
    pub evidence: Evidence,
}
pub trait Fetcher {
    fn get(&self, url: &str) -> Result<Document>;
}
pub struct HttpClient<T = Network> {
    pub options: CacheOptions,
    transport: T,
}
impl HttpClient<Network> {
    pub fn new(options: CacheOptions) -> Result<Self> {
        let mut transport = Network::new()?;
        transport.public_only = options.public_only;
        Ok(Self { options, transport })
    }
}
pub fn https(input: &str) -> Result<Url> {
    let url = Url::parse(input).map_err(|e| Error::Config(e.to_string()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Config(
            "sources must be credential-free HTTPS URLs".into(),
        ));
    }
    Ok(url)
}
fn github(url: &Url) -> bool {
    url.origin().ascii_serialization() == "https://api.github.com"
}
impl<T: Transport> Fetcher for HttpClient<T> {
    fn get(&self, input: &str) -> Result<Document> {
        self.request(input, None)
    }
}
impl<T: Transport> HttpClient<T> {
    pub fn marketplace_query(&self, id: &str) -> Result<Document> {
        let body = serde_json::json!({"filters":[{"criteria":[{"filterType":7,"value":id},{"filterType":8,"value":"Microsoft.VisualStudio.Code"}],"pageNumber":1,"pageSize":1}],"flags":147});
        self.request(
            "https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery",
            Some(body.to_string()),
        )
    }
    fn request(&self, input: &str, body: Option<String>) -> Result<Document> {
        if self.options.public_only && crate::icons::safe_url(input).is_none() {
            return Err(Error::Source("unsafe discovered source URL".into()));
        }
        let mut url = https(input)?;
        let scope = if github(&url) {
            self.options.github_token.as_deref().unwrap_or("")
        } else {
            ""
        };
        let key = format!(
            "{:x}",
            Sha256::digest(format!(
                "{input}\n{scope}{}{}",
                if self.options.public_only {
                    "\npublic"
                } else {
                    ""
                },
                body.as_ref()
                    .map(|b| format!("\nPOST\n{b}"))
                    .unwrap_or_default()
            ))
        );
        let cached = http_cache::load(&self.options.directory, &key, input);
        let now = Utc::now();
        if let Some(entry) = cached.as_ref() {
            let age = now.signed_duration_since(entry.fetched_at).num_seconds();
            if self.options.offline
                || (!self.options.refresh && age >= 0 && age < self.options.ttl.as_secs() as i64)
            {
                return Ok(document(
                    entry,
                    if self.options.offline {
                        "offline_cache"
                    } else {
                        "fresh_cache"
                    },
                ));
            }
        }
        if self.options.offline {
            return Err(Error::Offline(input.into()));
        }
        let rate_key = format!(
            "{:x}",
            Sha256::digest(format!("{}\n{scope}", url.origin().ascii_serialization()))
        );
        let rate_path = self.options.directory.join(format!("rate-{rate_key}.json"));
        if let Some((until, status)) = fs::File::open(&rate_path)
            .ok()
            .and_then(|f| serde_json::from_reader::<_, (i64, u16)>(f.take(128)).ok())
            && until > now.timestamp()
        {
            if let Some(entry) = &cached {
                return Ok(document(entry, "stale_cache_backoff"));
            }
            return Err(Error::Http {
                status,
                url: input.into(),
                detail: format!("; upstream cooldown until {until}"),
            });
        }
        for redirect in 0..=3 {
            let mut headers = BTreeMap::new();
            if body.is_some() {
                headers.insert("content-type".into(), "application/json".into());
                headers.insert(
                    "accept".into(),
                    "application/json;api-version=3.0-preview.1".into(),
                );
            }
            if github(&url) {
                headers.insert("accept".into(), "application/vnd.github+json".into());
                headers.insert("x-github-api-version".into(), "2026-03-10".into());
                if let Some(token) = self
                    .options
                    .github_token
                    .as_ref()
                    .filter(|_| !scope.is_empty())
                {
                    headers.insert("authorization".into(), format!("Bearer {token}"));
                }
            }
            if let Some(entry) = cached
                .as_ref()
                .filter(|e| e.effective_url.as_deref().unwrap_or(&e.url) == url.as_str())
            {
                for (source, target) in [
                    ("etag", "if-none-match"),
                    ("last-modified", "if-modified-since"),
                ] {
                    if let Some(value) = entry.headers.get(source) {
                        headers.insert(target.into(), value.clone());
                    }
                }
            }
            let response = self.transport.send(Request {
                url: url.clone(),
                headers,
                body: body.clone(),
            });
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    if let Some(entry) = &cached {
                        return Ok(document(entry, "stale_cache_network_error"));
                    }
                    return Err(error);
                }
            };
            if response.body.len() as u64 > MAX_BODY {
                return Err(Error::Source("response exceeds 8 MiB".into()));
            }
            if response.status == 304 {
                let mut entry =
                    cached.ok_or_else(|| Error::Source("304 without cached body".into()))?;
                entry.fetched_at = now;
                for (k, v) in response.headers {
                    if matches!(k.as_str(), "etag" | "last-modified" | "link") {
                        entry.headers.insert(k, v);
                    }
                }
                http_cache::store(&self.options.directory, &key, &entry)?;
                return Ok(document(&entry, "revalidated_cache"));
            }
            if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                if body.is_some() {
                    return Err(Error::Source("Marketplace query redirected".into()));
                }
                let location = response
                    .headers
                    .get("location")
                    .ok_or_else(|| Error::Source("redirect without Location".into()))?;
                let next = url
                    .join(location)
                    .map_err(|e| Error::Source(e.to_string()))?;
                https(next.as_str())?;
                if redirect == 3
                    || (next.origin() != url.origin() && !self.options.public_only)
                    || (self.options.public_only && crate::icons::safe_url(next.as_str()).is_none())
                {
                    return Err(Error::Source(
                        "cross-origin or excessive redirect; configure canonical URL".into(),
                    ));
                }
                url = next;
                continue;
            }
            if response.status != 200 {
                let limited = response.status == 429
                    || (response.status == 403
                        && response
                            .headers
                            .get("x-ratelimit-remaining")
                            .is_some_and(|s| s == "0"));
                if limited {
                    let wait = response
                        .headers
                        .get("retry-after")
                        .and_then(|s| {
                            s.parse::<i64>().ok().or_else(|| {
                                DateTime::parse_from_rfc2822(s)
                                    .ok()
                                    .map(|d| d.timestamp() - now.timestamp())
                            })
                        })
                        .or_else(|| {
                            response
                                .headers
                                .get("x-ratelimit-reset")
                                .and_then(|s| s.parse::<i64>().ok())
                                .map(|t| t - now.timestamp())
                        })
                        .unwrap_or(60)
                        .clamp(1, 86400);
                    fs::create_dir_all(&self.options.directory)?;
                    fs::write(
                        &rate_path,
                        serde_json::to_vec(&(now.timestamp() + wait, response.status))?,
                    )?;
                }
                if (response.status == 429 || response.status == 403 || response.status >= 500)
                    && let Some(entry) = &cached
                {
                    return Ok(document(entry, "stale_cache_upstream_error"));
                }
                let detail = if response.status == 403 || response.status == 429 {
                    if github(&url) {
                        "; access/rate limit, retry later or provide GITHUB_TOKEN"
                    } else {
                        "; access/rate limit, retry later"
                    }
                } else {
                    ""
                };
                return Err(Error::Http {
                    status: response.status,
                    url: input.into(),
                    detail: detail.into(),
                });
            }
            // Cache only validators and pagination, never credentials or cookies.
            let headers = response
                .headers
                .into_iter()
                .filter(|(k, _)| matches!(k.as_str(), "etag" | "last-modified" | "link"))
                .collect();
            let entry = Entry {
                url: input.into(),
                effective_url: Some(url.to_string()),
                fetched_at: now,
                headers,
                body: response.body,
            };
            http_cache::store(&self.options.directory, &key, &entry)?;
            return Ok(document(&entry, "network"));
        }
        unreachable!()
    }
}
fn document(entry: &Entry, cache: &str) -> Document {
    Document {
        body: entry.body.clone(),
        headers: entry.headers.clone(),
        evidence: Evidence {
            url: entry
                .effective_url
                .clone()
                .unwrap_or_else(|| entry.url.clone()),
            fetched_at: entry.fetched_at.to_rfc3339(),
            cache: cache.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Fake {
        replies: RefCell<Vec<Response>>,
        requests: RefCell<Vec<Request>>,
    }
    impl Transport for Fake {
        fn send(&self, r: Request) -> Result<Response> {
            self.requests.borrow_mut().push(r);
            Ok(self.replies.borrow_mut().remove(0))
        }
    }
    fn response(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
        Response {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.into(),
        }
    }
    #[test]
    fn discovered_http_cache_is_separate_and_private_urls_never_reach_transport() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![
                    response(200, &[], "ordinary"),
                    response(200, &[], "public"),
                ]),
                requests: RefCell::new(vec![]),
            },
        };
        assert_eq!(
            client.get("https://example.com/notes").unwrap().body,
            "ordinary"
        );
        client.options.public_only = true;
        client.options.offline = true;
        assert!(client.get("https://example.com/notes").is_err());
        client.options.offline = false;
        assert_eq!(
            client.get("https://example.com/notes").unwrap().body,
            "public"
        );
        assert!(client.get("https://127.0.0.1/notes").is_err());
        assert!(client.get("https://example.com:8443/notes").is_err());
        assert_eq!(client.transport.requests.borrow().len(), 2);
    }
    #[test]
    fn marketplace_post_cache_is_scoped_to_the_exact_extension_query() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                github_token: Some("secret".into()),
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![
                    response(200, &[], "first"),
                    response(200, &[], "second"),
                ]),
                requests: RefCell::new(vec![]),
            },
        };
        assert_eq!(
            client.marketplace_query("vendor.first").unwrap().body,
            "first"
        );
        assert_eq!(
            client.marketplace_query("vendor.first").unwrap().body,
            "first"
        );
        assert_eq!(
            client.marketplace_query("vendor.second").unwrap().body,
            "second"
        );
        let requests = client.transport.requests.borrow();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].body.as_ref().unwrap().contains("vendor.first"));
        assert!(
            requests
                .iter()
                .all(|r| !r.headers.contains_key("authorization"))
        );
        drop(requests);
        client.options.offline = true;
        assert_eq!(
            client.marketplace_query("vendor.first").unwrap().body,
            "first"
        );
        assert!(client.marketplace_query("vendor.other").is_err());
    }
    #[test]
    fn cache_revalidation_offline_and_token_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                github_token: Some("secret".into()),
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![
                    response(200, &[("etag", "abc")], "body"),
                    response(304, &[], ""),
                ]),
                requests: RefCell::new(vec![]),
            },
        };
        let url = "https://api.github.com/repos/test/test/releases";
        assert_eq!(client.get(url).unwrap().evidence.cache, "network");
        assert_eq!(client.get(url).unwrap().evidence.cache, "fresh_cache");
        client.options.refresh = true;
        assert_eq!(client.get(url).unwrap().evidence.cache, "revalidated_cache");
        assert_eq!(
            client.transport.requests.borrow()[1].headers["if-none-match"],
            "abc"
        );
        client.options.offline = true;
        assert_eq!(client.get(url).unwrap().body, "body");
        client.options.github_token = Some("other".into());
        assert!(matches!(client.get(url), Err(Error::Offline(_))));
        client.options.offline = false;
        client
            .transport
            .replies
            .borrow_mut()
            .push(response(200, &[], "x"));
        client.get("https://example.com/notes").unwrap();
        assert!(
            !client
                .transport
                .requests
                .borrow()
                .last()
                .unwrap()
                .headers
                .contains_key("authorization")
        );
    }
    #[test]
    fn rejects_redirect_leaks_and_bad_urls() {
        let dir = tempfile::tempdir().unwrap();
        let client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![response(
                    302,
                    &[("location", "https://evil.example/notes")],
                    "",
                )]),
                requests: RefCell::new(vec![]),
            },
        };
        assert!(client.get("http://example.com").is_err());
        assert!(client.get("https://example.com").is_err());
        assert_eq!(client.transport.requests.borrow().len(), 1);
    }
    #[test]
    fn stale_cache_corruption_limits_and_status_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                ttl: Duration::ZERO,
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![
                    response(200, &[], "cached"),
                    response(429, &[], "limited"),
                    response(304, &[], ""),
                ]),
                requests: RefCell::new(vec![]),
            },
        };
        let url = "https://example.com/notes";
        client.get(url).unwrap();
        assert_eq!(
            client.get(url).unwrap().evidence.cache,
            "stale_cache_upstream_error"
        );
        assert_eq!(
            client.get(url).unwrap().evidence.cache,
            "stale_cache_backoff"
        );
        client.options.offline = true;
        assert_eq!(client.get(url).unwrap().evidence.cache, "offline_cache");
        let path = fs::read_dir(dir.path())
            .unwrap()
            .find(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".json.gz")
            })
            .unwrap()
            .unwrap()
            .path();
        fs::write(path, "corrupt").unwrap();
        assert!(matches!(client.get(url), Err(Error::Offline(_))));
        client.options.offline = false;
        for entry in fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("rate-"))
        {
            fs::remove_file(entry.path()).unwrap();
        }
        assert!(
            client
                .get(url)
                .unwrap_err()
                .to_string()
                .contains("304 without cached")
        );
        client.transport.replies.borrow_mut().push(response(
            200,
            &[],
            &"x".repeat(MAX_BODY as usize + 1),
        ));
        assert!(
            client
                .get(url)
                .unwrap_err()
                .to_string()
                .contains("exceeds 8 MiB")
        );
    }

    #[test]
    fn public_redirects_are_canonical_but_never_forward_credentials_or_allow_private_targets() {
        let dir = tempfile::tempdir().unwrap();
        let client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                public_only: true,
                github_token: Some("secret".into()),
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![
                    response(302, &[("location", "https://example.com/notes")], ""),
                    response(200, &[], "notes"),
                ]),
                requests: RefCell::new(vec![]),
            },
        };
        let doc = client
            .get("https://api.github.com/repos/a/b/releases")
            .unwrap();
        assert_eq!(doc.evidence.url, "https://example.com/notes");
        let requests = client.transport.requests.borrow();
        assert!(requests[0].headers.contains_key("authorization"));
        assert!(!requests[1].headers.contains_key("authorization"));
        drop(requests);
        let client = HttpClient {
            options: CacheOptions {
                directory: dir.path().into(),
                public_only: true,
                ..Default::default()
            },
            transport: Fake {
                replies: RefCell::new(vec![response(
                    302,
                    &[("location", "https://127.0.0.1/private")],
                    "",
                )]),
                requests: RefCell::new(vec![]),
            },
        };
        assert!(client.get("https://other.example/start").is_err());
        assert_eq!(client.transport.requests.borrow().len(), 1);
    }
}
