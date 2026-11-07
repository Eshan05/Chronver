use crate::{Error, Result, Scheme};
use std::cmp::Ordering;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Version {
    Semver(semver::Version),
    Numeric(Vec<u64>, semver::Prerelease),
}

impl Version {
    pub fn parse(input: &str, scheme: Scheme) -> Result<Self> {
        let input = input.trim().strip_prefix('v').unwrap_or(input.trim());
        let fail = || Error::Version(format!("{input:?} is not a supported {scheme:?} version"));
        match scheme {
            Scheme::Semver => {
                let mut v = semver::Version::parse(input).map_err(|_| fail())?;
                // Build metadata does not affect SemVer precedence.
                v.build = semver::BuildMetadata::EMPTY;
                Ok(Self::Semver(v))
            }
            Scheme::Numeric => {
                let input = if let Some((version, metadata)) = input.split_once('+') {
                    semver::BuildMetadata::new(metadata).map_err(|_| fail())?;
                    if metadata.is_empty() {
                        return Err(fail());
                    }
                    version
                } else {
                    input
                };
                let (core, pre) = input.split_once('-').unwrap_or((input, ""));
                if input.contains('-') && pre.is_empty() {
                    return Err(fail());
                }
                let pre = semver::Prerelease::new(pre).map_err(|_| fail())?;
                let mut parts = core
                    .split('.')
                    .map(|p| {
                        if p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) {
                            return Err(fail());
                        }
                        p.parse::<u64>().map_err(|_| fail())
                    })
                    .collect::<Result<Vec<_>>>()?;
                if !(1..=4).contains(&parts.len()) {
                    return Err(fail());
                }
                while parts.len() > 1 && parts.last() == Some(&0) {
                    parts.pop();
                }
                Ok(Self::Numeric(parts, pre))
            }
        }
    }
    pub fn preview(&self) -> bool {
        match self {
            Self::Semver(v) => !v.pre.is_empty(),
            Self::Numeric(_, pre) => !pre.is_empty(),
        }
    }
    pub fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Semver(a), Self::Semver(b)) => a.cmp(b),
            (Self::Numeric(a, ap), Self::Numeric(b, bp)) => a.cmp(b).then_with(|| ap.cmp(bp)),
            _ => unreachable!("a stream has exactly one version scheme"),
        }
    }
}

fn revision(v: &str) -> Option<&str> {
    v.strip_prefix('r')
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn package_revision(v: &str) -> Option<(Version, u64)> {
    let (core, rev) = v.rsplit_once('_')?;
    if rev.is_empty() || !rev.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((
        Version::parse(core, Scheme::Numeric).ok()?,
        rev.parse().ok()?,
    ))
}

fn git_build(v: &str) -> Option<(Version, &str)> {
    let (core, hash) = v.rsplit_once(".g")?;
    if !(4..=40).contains(&hash.len()) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((Version::parse(core, Scheme::Numeric).ok()?, hash))
}

/// A total order for displaying mixed inventory versions, not evidence of an upgrade.
pub fn inventory_sort_key(input: &str) -> impl Ord + '_ {
    type Numeric = (Vec<u64>, semver::Prerelease);
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum Key<'a> {
        Numeric(Numeric),
        Revision(u64),
        PackageRevision(Numeric, u64),
        GitBuild(Numeric, &'a str),
        Label(&'a str),
    }
    let input = input.trim();
    if let Ok(Version::Numeric(parts, pre)) = Version::parse(input, Scheme::Numeric) {
        Key::Numeric((parts, pre))
    } else if let Some(rev) = revision(input).and_then(|n| n.parse().ok()) {
        Key::Revision(rev)
    } else if let Some((Version::Numeric(parts, pre), rev)) = package_revision(input) {
        Key::PackageRevision((parts, pre), rev)
    } else if let Some((Version::Numeric(parts, pre), hash)) = git_build(input) {
        Key::GitBuild((parts, pre), hash)
    } else {
        Key::Label(input)
    }
}

/// Inventory precedence, without relaxing a release stream's configured version scheme.
/// Returns the candidate's order relative to the installed version; unknown stays unknown.
pub fn candidate_order(installed: &str, candidate: &str) -> Option<Ordering> {
    fn numeric_order(from: &str, to: &str) -> Option<Ordering> {
        [Scheme::Semver, Scheme::Numeric]
            .into_iter()
            .find_map(|scheme| {
                let from = Version::parse(from, scheme).ok()?;
                let to = Version::parse(to, scheme).ok()?;
                Some(to.compare(&from))
            })
    }
    let installed = installed.trim();
    let candidate = candidate.trim();
    if let Some(order) = numeric_order(installed, candidate) {
        return Some(order);
    }
    // WinGet may know only an upper bound, not the exact installed version.
    let bound = installed
        .strip_prefix("<=")
        .map(|v| (v.trim(), false))
        .or_else(|| installed.strip_prefix('<').map(|v| (v.trim(), true)));
    if let Some((bound, exclusive)) = bound {
        let order = numeric_order(bound, candidate)?;
        return (order.is_gt() || (exclusive && order.is_eq())).then_some(Ordering::Greater);
    }
    if let (Some(from), Some(to)) = (revision(installed), revision(candidate)) {
        return numeric_order(from, to);
    }
    if let (Some((from, fr)), Some((to, tr))) =
        (package_revision(installed), package_revision(candidate))
    {
        return Some(to.compare(&from).then_with(|| tr.cmp(&fr)));
    }
    if let (Some((from, fh)), Some((to, th))) = (git_build(installed), git_build(candidate)) {
        let order = to.compare(&from);
        return (!order.is_eq() || fh == th).then_some(order);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_sort_orders_mixed_formats_without_inferring_updates() {
        let mut versions = [
            "1x",
            "r100",
            "2.0",
            "1.4.1_10",
            "< 9.0",
            "4.0",
            "r99",
            "1.2.3.5.g806b5b27",
            "10.0",
            "1.2.3.4.g85e641b4",
            "4.0-rc.2",
            "1.4.1_2",
        ];
        versions.sort_by_cached_key(|&version| inventory_sort_key(version));
        assert_eq!(
            versions,
            [
                "2.0",
                "4.0-rc.2",
                "4.0",
                "10.0",
                "r99",
                "r100",
                "1.4.1_2",
                "1.4.1_10",
                "1.2.3.4.g85e641b4",
                "1.2.3.5.g806b5b27",
                "1x",
                "< 9.0",
            ]
        );
        assert_eq!(candidate_order("2.0", "1x"), None);
    }

    #[test]
    fn inventory_formats_have_conservative_precedence_without_guessing_hashes_or_bounds() {
        for (from, to, expected) in [
            ("r40", "r42", Some(Ordering::Greater)),
            ("r100", "r99", Some(Ordering::Less)),
            ("r40", "r40", Some(Ordering::Equal)),
            ("r40", "42", None),
            ("r40", "r42-beta", None),
            ("8.18.0_2", "8.23.0_9", Some(Ordering::Greater)),
            ("8.18.0_2", "8.18.0_10", Some(Ordering::Greater)),
            ("8.18.0_2", "8.18.0_b", None),
            ("< 3.14.7", "3.15.2", Some(Ordering::Greater)),
            ("< 3.14.7", "3.14.7", Some(Ordering::Greater)),
            ("< 3.14.7", "3.14.6", None),
            ("<= 3.14.7", "3.14.7", None),
            ("<= 3.14.7", "3.15.2", Some(Ordering::Greater)),
            (
                "1.2.65.255.g85e641b4",
                "1.3.9.2055.g806b5b278de8",
                Some(Ordering::Greater),
            ),
            ("1.2.65.255.g85e641b4", "1.2.65.255.g806b5b27", None),
            (
                "1.2.65.255.g85e641b4",
                "1.2.65.255.g85e641b4",
                Some(Ordering::Equal),
            ),
            ("nightly", "nightly", None),
            ("abcd1234", "abcd5678", None),
            ("v1.2.3", "1.3.0", Some(Ordering::Greater)),
            ("1.2.3+one", "1.2.3+two", Some(Ordering::Equal)),
        ] {
            assert_eq!(candidate_order(from, to), expected, "{from} -> {to}");
        }
        assert!(Version::parse("r42", Scheme::Numeric).is_err());
    }
    #[test]
    fn numeric_is_not_lexical_and_handles_padding() {
        let a = Version::parse("2026.09.2", Scheme::Numeric).unwrap();
        let b = Version::parse("2026.10.1", Scheme::Numeric).unwrap();
        assert_eq!(a.compare(&b), Ordering::Less);
        assert_eq!(
            Version::parse("1.2.0.0", Scheme::Numeric).unwrap(),
            Version::parse("1.2", Scheme::Numeric).unwrap()
        );
        assert!(Version::parse("1.2beta", Scheme::Numeric).is_err());
        assert!(Version::parse("1.2.3.4.5", Scheme::Numeric).is_err());
        let preview = Version::parse("1.2.0.0-rc.2", Scheme::Numeric).unwrap();
        assert!(preview.preview());
        assert_eq!(
            preview.compare(&Version::parse("1.2", Scheme::Numeric).unwrap()),
            Ordering::Less
        );
    }
    #[test]
    fn semver_metadata_is_not_an_upgrade() {
        assert_eq!(
            Version::parse("v1.2.3+one", Scheme::Semver).unwrap(),
            Version::parse("1.2.3+two", Scheme::Semver).unwrap()
        );
        assert!(
            Version::parse("1.2.3-rc.1", Scheme::Semver)
                .unwrap()
                .preview()
        );
        assert!(Version::parse("nightly", Scheme::Semver).is_err());
    }
}
