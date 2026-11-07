//! Product icon discovery. No owner avatars, favicon proxies, or inventory uploads.
use crate::{Error, Result};
use image::{ImageFormat, ImageReader, Limits};
use reqwest::{blocking::Client, redirect::Policy};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    net::{IpAddr, ToSocketAddrs},
    path::{Path, PathBuf},
    time::Duration,
};
use url::Url;

const MAX: u64 = 1024 * 1024;
const DISCOVERY: u8 = 2;
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Plan {
    pub icon_url: Option<String>,
    pub homepage: Option<String>,
}
impl Plan {
    pub fn is_empty(&self) -> bool {
        self.icon_url.is_none() && self.homepage.is_none()
    }
}
pub struct Reply {
    pub url: Url,
    pub body: Vec<u8>,
}
pub trait Fetch {
    fn get(&self, url: &Url) -> Result<Reply>;
}
pub struct Network;

pub fn safe_url(input: &str) -> Option<Url> {
    if input.len() > 2048 {
        return None;
    }
    let u = Url::parse(input).ok()?;
    let host = u.host_str()?;
    (u.scheme() == "https"
        && u.username().is_empty()
        && u.password().is_none()
        && u.port_or_known_default() == Some(443)
        && host.contains('.')
        && !host.ends_with(".local")
        && !host.ends_with(".localhost")
        && host.parse::<IpAddr>().is_err()
        && !host.starts_with('['))
    .then_some(u)
}
pub fn product_homepage(input: &str) -> Option<String> {
    let u = safe_url(input)?;
    let host = u.host_str()?.trim_start_matches("www.");
    if [
        "github.com",
        "raw.githubusercontent.com",
        "api.github.com",
        "npmjs.com",
        "npmjs.org",
        "pypi.org",
        "gitlab.com",
        "bitbucket.org",
    ]
    .iter()
    .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        return None;
    }
    Some(u.into())
}
pub(crate) fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_unspecified()
                || v.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19)))
        }
        IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v| public(IpAddr::V4(v)))
            .unwrap_or_else(|| {
                let s = v.segments();
                s[0] & 0xe000 == 0x2000
                    && s[0] != 0x2002
                    && !(s[0] == 0x2001 && (s[1] == 0x0db8 || s[1] < 0x0200))
            }),
    }
}

impl Fetch for Network {
    fn get(&self, input: &Url) -> Result<Reply> {
        let started = std::time::Instant::now();
        let mut url = input.clone();
        for _ in 0..4 {
            if started.elapsed() > Duration::from_secs(15) {
                return Err(Error::Source("icon request deadline exceeded".into()));
            }
            url = safe_url(url.as_str()).ok_or_else(|| Error::Source("unsafe icon URL".into()))?;
            let host = url.host_str().unwrap();
            let addresses: Vec<_> = (host, 443).to_socket_addrs()?.collect();
            if addresses.is_empty() || addresses.iter().any(|a| !public(a.ip())) {
                return Err(Error::Source("non-public icon host".into()));
            }
            // Pin validated addresses and do not use ambient HTTP proxies. Each
            // redirect is validated again, without sending credentials or cookies.
            let client = Client::builder()
                .no_proxy()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(5))
                .connect_timeout(Duration::from_secs(3))
                .user_agent(format!("{} (product icons)", crate::USER_AGENT))
                .resolve_to_addrs(host, &addresses)
                .build()?;
            let response = client.get(url.clone()).send()?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|s| s.to_str().ok())
                    .ok_or_else(|| Error::Source("icon redirect without location".into()))?;
                url = url
                    .join(location)
                    .map_err(|e| Error::Source(e.to_string()))?;
                continue;
            }
            let response = response.error_for_status()?;
            let html = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';')
                        .next()
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("text/html"))
                });
            if !html && response.content_length().is_some_and(|n| n > MAX) {
                return Err(Error::Source("icon response too large".into()));
            }
            let mut body = vec![];
            response.take(MAX + 1).read_to_end(&mut body)?;
            if body.len() as u64 > MAX {
                if html {
                    body.truncate(MAX as usize);
                } else {
                    return Err(Error::Source("icon response too large".into()));
                }
            }
            return Ok(Reply { url, body });
        }
        Err(Error::Source("too many icon redirects".into()))
    }
}
/// Validate and normalize a local icon with the same bounded raster decoder as remote icons.
pub fn normalize_icon(bytes: &[u8]) -> Result<Vec<u8>> {
    raster(bytes).ok_or_else(|| {
        Error::Source(
            "Choose a PNG, JPEG, WebP or ICO image up to 1 MiB and 1024 x 1024 pixels".into(),
        )
    })
}

fn raster(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() as u64 > MAX {
        return None;
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Ico | ImageFormat::WebP)
    ) {
        return None;
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(96, 96);
    let mut output = Cursor::new(vec![]);
    image.write_to(&mut output, ImageFormat::Png).ok()?;
    Some(output.into_inner())
}

fn svg(bytes: &[u8]) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    if bytes.len() > 256 * 1024 {
        return None;
    }
    let input = std::str::from_utf8(bytes).ok()?;
    if !input
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
    {
        return None;
    }
    // Static vectors only: never load referenced files, URLs, embedded images or fonts.
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(input, &options).ok()?;
    let size = tree.size();
    if size.width() > 1024. || size.height() > 1024. || !tree.filters().is_empty() {
        return None;
    }
    let mut groups = vec![tree.root()];
    groups.extend(tree.patterns().iter().map(|p| p.root()));
    groups.extend(tree.clip_paths().iter().map(|p| p.root()));
    groups.extend(tree.masks().iter().map(|p| p.root()));
    let mut nodes = 0;
    while let Some(group) = groups.pop() {
        if !group.filters().is_empty() {
            return None;
        }
        for node in group.children() {
            nodes += 1;
            if nodes > 4096 {
                return None;
            }
            if let usvg::Node::Group(group) = node {
                groups.push(group);
            }
        }
    }
    let scale = 96. / size.width().max(size.height());
    let mut image = tiny_skia::Pixmap::new(
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
    )?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut image.as_mut(),
    );
    if !image.pixels().iter().any(|p| p.alpha() > 0) {
        return None;
    }
    image.encode_png().ok()
}
fn links(page: &str, base: &Url) -> Vec<Url> {
    let doc = Html::parse_document(page);
    let selector = Selector::parse("link[rel][href]").unwrap();
    let mut found = vec![];
    for el in doc.select(&selector) {
        if !el
            .value()
            .attr("rel")
            .unwrap_or("")
            .split_ascii_whitespace()
            .any(|r| r.eq_ignore_ascii_case("icon") || r.eq_ignore_ascii_case("apple-touch-icon"))
        {
            continue;
        }
        if let Some(u) = el
            .value()
            .attr("href")
            .and_then(|h| base.join(h).ok())
            .and_then(|u| safe_url(u.as_str()))
            && !found.contains(&u)
        {
            found.push(u);
        }
        if found.len() == 4 {
            break;
        }
    }
    found
}
#[derive(Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    discovery: u8,
    plan: Plan,
    checked: i64,
    source: Option<String>,
}
pub struct Resolved {
    pub png: Vec<u8>,
    pub source: String,
}
pub enum Cached {
    Hit { resolved: Resolved, fresh: bool },
    Negative,
    Miss,
}
pub struct Resolver<T = Network> {
    pub directory: PathBuf,
    pub fetch: T,
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temp.write_all(bytes)?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
impl<T> Resolver<T> {
    fn paths(&self, plan: &Plan) -> Option<(PathBuf, PathBuf)> {
        let key = format!("{:x}", Sha256::digest(serde_json::to_vec(plan).ok()?));
        Some((
            self.directory.join(format!("{key}.json")),
            self.directory.join(format!("{key}.png")),
        ))
    }

    /// Disk only. Expired positive entries remain usable while a separate worker refreshes them.
    pub fn cached(&self, plan: &Plan) -> Cached {
        if plan.is_empty() {
            return Cached::Miss;
        }
        let Some((meta, png)) = self.paths(plan) else {
            return Cached::Miss;
        };
        let now = chrono::Utc::now().timestamp();
        let entry = fs::File::open(&meta)
            .ok()
            .and_then(|f| serde_json::from_reader::<_, Entry>(f.take(8192)).ok());
        if let Some(e) =
            entry.filter(|e| e.plan == *plan && (e.source.is_some() || e.discovery == DISCOVERY))
        {
            let age = now - e.checked;
            let fresh = (0..if e.source.is_some() {
                30 * 86400
            } else {
                86400
            })
                .contains(&age);
            if let Some(source) = e.source {
                let data = fs::File::open(&png).ok().and_then(|f| {
                    let mut b = vec![];
                    f.take(MAX + 1).read_to_end(&mut b).ok()?;
                    raster(&b)
                });
                if let Some(png) = data {
                    return Cached::Hit {
                        resolved: Resolved { png, source },
                        fresh,
                    };
                }
            } else if fresh {
                return Cached::Negative;
            }
        }
        Cached::Miss
    }
}
impl<T: Fetch> Resolver<T> {
    pub fn resolve(&self, plan: &Plan) -> Option<Resolved> {
        if plan.is_empty() {
            return None;
        }
        let stale = match self.cached(plan) {
            Cached::Hit {
                resolved,
                fresh: true,
            } => return Some(resolved),
            Cached::Hit {
                resolved,
                fresh: false,
            } => Some(resolved),
            Cached::Negative => return None,
            Cached::Miss => None,
        };
        let (meta, png) = self.paths(plan)?;
        let result = self.discover(plan);
        if result.is_none() && stale.is_some() {
            return stale;
        }
        if let Some(r) = &result {
            let _ = atomic(&png, &r.png);
        }
        let _ = atomic(
            &meta,
            &serde_json::to_vec(&Entry {
                discovery: DISCOVERY,
                plan: plan.clone(),
                checked: chrono::Utc::now().timestamp(),
                source: result.as_ref().map(|r| r.source.clone()),
            })
            .ok()?,
        );
        result
    }
    fn discover(&self, plan: &Plan) -> Option<Resolved> {
        let started = std::time::Instant::now();
        let get = |url: &Url| {
            if started.elapsed() > Duration::from_secs(25) {
                return None;
            }
            let r = self.fetch.get(url).ok()?;
            Some(Resolved {
                png: raster(&r.body).or_else(|| svg(&r.body))?,
                source: r.url.into(),
            })
        };
        if let Some(url) = plan.icon_url.as_deref().and_then(safe_url)
            && let Some(r) = get(&url)
        {
            return Some(r);
        }
        let home = plan
            .homepage
            .as_deref()
            .and_then(product_homepage)
            .and_then(|s| Url::parse(&s).ok())?;
        if let Ok(page) = self.fetch.get(&home)
            && product_homepage(page.url.as_str()).is_some()
        {
            let html = String::from_utf8_lossy(&page.body);
            for url in links(&html, &page.url) {
                if let Some(r) = get(&url) {
                    return Some(r);
                }
            }
            if let Ok(url) = page.url.join("/favicon.ico") {
                return get(&url);
            }
        }
        home.join("/favicon.ico").ok().and_then(|u| get(&u))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_vectors_become_bounded_pngs_without_external_resources() {
        let vector = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12"><path d="M0 0h24v12H0z" fill="#00ff00"/></svg>"##;
        let png = svg(vector).unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (96, 48));
        assert_eq!(image.get_pixel(40, 20).0, [0, 255, 0, 255]);
        assert!(
            svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="5000" height="5000"/>"#)
                .is_none()
        );
        assert!(svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><image href="file:///C:/secret.png" width="24" height="24"/></svg>"#).is_none());
        assert!(svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><image href="https://private.example/icon.png" width="24" height="24"/></svg>"#).is_none());
        assert!(svg(br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><filter id="f"><feGaussianBlur stdDeviation="2"/></filter><g filter="url(#f)"><rect width="24" height="24"/></g></svg>"##).is_none());
        assert!(svg(&vec![b'<'; 256 * 1024 + 1]).is_none());
        assert!(svg(b"<svg").is_none());
    }
    #[test]
    fn private_and_transition_addresses_cannot_be_icon_servers() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "192.168.1.1",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:127.0.0.1",
            "2002:7f00:1::1",
            "2001:db8::1",
        ] {
            assert!(!public(ip.parse().unwrap()), "{ip}");
        }
        assert!(public("8.8.8.8".parse().unwrap()));
        assert!(public("2606:4700:4700::1111".parse().unwrap()));
    }
}
