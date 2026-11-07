//! Command preparation only. This module never spawns a process or changes software.
use releasescout_engine::inventory::{InstalledPackage, Manager};

#[derive(Clone)]
pub struct Plan {
    pub record: InstalledPackage,
}

fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        && s.as_bytes()[0].is_ascii_alphanumeric()
}

fn npm_id(s: &str) -> bool {
    if let Some(s) = s.strip_prefix('@') {
        s.split_once('/')
            .is_some_and(|(scope, name)| token(scope) && token(name))
    } else {
        token(s)
    }
}

impl Plan {
    pub fn supports(record: &InstalledPackage) -> bool {
        let supported = match record.manager {
            Manager::Npm | Manager::Pnpm | Manager::Bun => npm_id(&record.id),
            Manager::Chocolatey => token(&record.id) && !record.id.eq_ignore_ascii_case("all"),
            Manager::Winget | Manager::Scoop => token(&record.id),
            _ => false,
        };
        supported
            && !record.pinned
            && record.installed.is_some()
            && record.available.is_some()
            && (!matches!(record.manager, Manager::Winget | Manager::Scoop)
                || (token(&record.source) && !record.source.eq_ignore_ascii_case("unknown")))
    }

    pub fn new(record: &InstalledPackage) -> Option<Self> {
        Self::supports(record).then(|| Self {
            record: record.clone(),
        })
    }

    pub fn specific_version(&self) -> bool {
        self.record.manager != Manager::Scoop
    }

    pub fn command(&self, version: &str) -> Result<String, &'static str> {
        let p = &self.record;
        if !token(version) || !version.bytes().any(|b| b.is_ascii_digit()) {
            return Err("Enter a version using letters, digits, dots, dashes or plus signs.");
        }
        let newer = [
            releasescout_engine::Scheme::Semver,
            releasescout_engine::Scheme::Numeric,
        ]
        .into_iter()
        .any(|scheme| {
            use releasescout_engine::version::Version;
            Version::parse(p.installed.as_deref().unwrap_or(""), scheme)
                .ok()
                .zip(Version::parse(version, scheme).ok())
                .is_some_and(|(from, to)| to.compare(&from).is_gt())
        });
        if !newer {
            return Err("Choose a comparable version newer than the installed version.");
        }
        if !self.specific_version() && p.available.as_deref() != Some(version) {
            return Err(
                "Scoop update uses the current manifest; selecting an arbitrary version is not supported.",
            );
        }
        let id = &p.id;
        Ok(match p.manager {
            Manager::Winget => {
                if !token(&p.source) {
                    return Err("An exact WinGet source is required to prepare this command.");
                }
                format!(
                    "winget upgrade --id '{id}' --exact --source '{}' --version '{version}'",
                    p.source
                )
            }
            Manager::Chocolatey => format!("choco upgrade '{id}' --version '{version}'"),
            Manager::Scoop => {
                if !token(&p.source) {
                    return Err("An exact Scoop bucket is required to prepare this command.");
                }
                format!(
                    "scoop update '{}/{id}'{}",
                    p.source,
                    if matches!(p.scope.as_str(), "global" | "machine") {
                        " --global"
                    } else {
                        ""
                    }
                )
            }
            Manager::Npm => format!("npm install --global '{id}@{version}'"),
            Manager::Pnpm => format!("pnpm add --global '{id}@{version}'"),
            Manager::Bun => format!("bun add --global '{id}@{version}'"),
            _ => return Err("This provider has no upgrade command adapter."),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(manager: Manager) -> InstalledPackage {
        serde_json::from_value(
            serde_json::json!({"manager":manager,"id":"fixture","name":"Fixture",
            "installed":"1.0.0","available":"2.0.0","source":"main","scope":"user","root":null,
            "pinned":false,"install_origin_known":false}),
        )
        .unwrap()
    }
    #[test]
    fn commands_are_exact_bounded_and_do_not_accept_shell_or_registry_protocols() {
        for (manager, expected) in [
            (
                Manager::Winget,
                "winget upgrade --id 'fixture' --exact --source 'main' --version '2.0.0'",
            ),
            (Manager::Scoop, "scoop update 'main/fixture'"),
            (
                Manager::Chocolatey,
                "choco upgrade 'fixture' --version '2.0.0'",
            ),
            (Manager::Npm, "npm install --global 'fixture@2.0.0'"),
            (Manager::Pnpm, "pnpm add --global 'fixture@2.0.0'"),
            (Manager::Bun, "bun add --global 'fixture@2.0.0'"),
        ] {
            let p = Plan::new(&record(manager)).unwrap();
            assert_eq!(p.command("2.0.0").unwrap(), expected);
            for version in [
                "",
                "latest",
                "2.0; whoami",
                "$(whoami)",
                "'bad'",
                "--all",
                "1\n2",
                "file:foo",
            ] {
                assert!(p.command(version).is_err(), "{version}");
            }
        }
        let mut p = record(Manager::Pnpm);
        p.id = "@vendor/fixture".into();
        assert!(
            Plan::new(&p)
                .unwrap()
                .command("2.0.0")
                .unwrap()
                .contains("'@vendor/fixture@2.0.0'")
        );
        for id in [
            "--all",
            "all",
            "file:foo",
            "https://evil.test/pkg",
            "thing';whoami",
            "@scope/../bad",
        ] {
            p.id = id.into();
            if id != "all" {
                assert!(Plan::new(&p).is_none());
            }
        }
    }
    #[test]
    fn unsupported_pinned_or_unobserved_entries_have_no_plan() {
        for manager in [
            Manager::Local,
            Manager::Store,
            Manager::Vscode,
            Manager::VscodeInsiders,
            Manager::Cursor,
        ] {
            assert!(Plan::new(&record(manager)).is_none());
        }
        let mut r = record(Manager::Scoop);
        let p = Plan::new(&r).unwrap();
        assert!(!p.specific_version());
        assert!(p.command("1.9.0").is_err());
        r.pinned = true;
        assert!(Plan::new(&r).is_none());
        r.pinned = false;
        r.available = None;
        assert!(Plan::new(&r).is_none());
        r = record(Manager::Chocolatey);
        r.id = "all".into();
        assert!(Plan::new(&r).is_none());
        r = record(Manager::Scoop);
        r.scope = "machine".into();
        assert!(
            Plan::new(&r)
                .unwrap()
                .command("2.0.0")
                .unwrap()
                .ends_with(" --global")
        );
        let p = Plan::new(&record(Manager::Pnpm)).unwrap();
        assert!(p.command("0.9.0").is_err());
        assert!(p.command("1.0.0").is_err());
        assert!(p.command("2.0.0").is_ok());
    }
}
