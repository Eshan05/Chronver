//! Explicit read-only Windows/global inventory. No package installation or repair commands.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Manager {
    Winget,
    Scoop,
    Chocolatey,
    Npm,
    Pnpm,
    Bun,
    Store,
    VscodeInsiders,
    Local,
    Vscode,
    Cursor,
}
impl Manager {
    pub const ALL: [Self; 11] = [
        Self::Winget,
        Self::Scoop,
        Self::Chocolatey,
        Self::Npm,
        Self::Pnpm,
        Self::Bun,
        Self::Store,
        Self::VscodeInsiders,
        Self::Local,
        Self::Vscode,
        Self::Cursor,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Winget => "winget",
            Self::Scoop => "scoop",
            Self::Chocolatey => "chocolatey",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Bun => "bun",
            Self::Store => "store",
            Self::VscodeInsiders => "vscode-insiders",
            Self::Local => "local",
            Self::Vscode => "vscode",
            Self::Cursor => "cursor",
        }
    }
    pub const EDITORS: [Self; 3] = [Self::Vscode, Self::VscodeInsiders, Self::Cursor];
    pub fn is_editor(self) -> bool {
        Self::EDITORS.contains(&self)
    }
    pub fn editor_name(self) -> Option<&'static str> {
        match self {
            Self::Vscode => Some("VS Code"),
            Self::VscodeInsiders => Some("VS Code Insiders"),
            Self::Cursor => Some("Cursor"),
            _ => None,
        }
    }
}
impl std::str::FromStr for Manager {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        Self::ALL.into_iter().find(|m| m.id() == s).ok_or_else(|| {
            "choose winget, scoop, chocolatey, npm, pnpm, bun, store, vscode-insiders, local, vscode, or cursor"
                .into()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub manager: Manager,
    pub id: String,
    pub name: String,
    pub installed: Option<String>,
    pub available: Option<String>,
    /// Per-record result; None means unchecked or a legacy snapshot with scan-only status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_check_succeeded: Option<bool>,
    #[serde(default)]
    pub candidate_note: Option<String>,
    #[serde(default)]
    pub icon_path: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub repository_directory: Option<String>,
    #[serde(default)]
    pub release_notes: Option<String>,
    #[serde(default)]
    pub download_urls: Vec<String>,
    #[serde(default)]
    pub package_root: Option<String>,
    #[serde(default)]
    pub extension: Option<crate::extensions::Installed>,
    #[serde(default)]
    pub evidence: InventoryEvidence,
    pub source: String,
    pub scope: String,
    pub root: Option<String>,
    pub pinned: bool,
    /// Observation by a manager is not evidence of the original installer.
    pub install_origin_known: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryEvidence {
    pub font_only: Option<bool>,
    pub registration_id: Option<String>,
    pub non_removable: Option<bool>,
    pub signature_kind: Option<String>,
    pub has_visible_app: Option<bool>,
    pub has_execution_alias: Option<bool>,
    pub has_cli_alias: Option<bool>,
    pub is_development_mode: Option<bool>,
    #[serde(default)]
    pub package_tags: String,
    pub wrapper_target: Option<String>,
    pub publisher: Option<String>,
    pub is_inbox: Option<bool>,
    pub has_preinstalled_task: Option<bool>,
    pub system_component: Option<bool>,
    pub parent_package: Option<String>,
    pub release_type: Option<String>,
    pub description: Option<String>,
    pub has_commands: Option<bool>,
    pub has_shortcuts: Option<bool>,
    pub runtime_only: Option<bool>,
    pub library_only: Option<bool>,
    pub background_only: Option<bool>,
    pub support_data: Option<bool>,
    pub is_dependency: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scan {
    pub manager: Manager,
    pub checked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_checked_at: Option<String>,
    pub update_check_requested: bool,
    pub update_check_succeeded: bool,
    pub packages: Vec<InstalledPackage>,
    pub warnings: Vec<String>,
}
impl Scan {
    pub fn candidate_time(&self) -> &str {
        self.candidate_checked_at
            .as_deref()
            .unwrap_or(&self.checked_at)
    }
}
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub trait Runner {
    fn run(&self, script: &str) -> Result<CommandOutput>;
}
pub struct ProcessRunner {
    pub cancel: Arc<AtomicBool>,
    pub timeout: Duration,
}
impl Default for ProcessRunner {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            timeout: Duration::from_secs(60),
        }
    }
}
const LIMIT: usize = 8 * 1024 * 1024;
fn reader(mut pipe: impl Read, overflow: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut bytes = vec![];
    let mut buf = [0; 8192];
    loop {
        let count = pipe.read(&mut buf)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > LIMIT {
            overflow.store(true, Ordering::Relaxed);
            break;
        }
        bytes.extend_from_slice(&buf[..count]);
    }
    Ok(bytes)
}
impl Runner for ProcessRunner {
    fn run(&self, script: &str) -> Result<CommandOutput> {
        if !cfg!(windows) {
            return Err(Error::Config(
                "these inventory providers require Windows".into(),
            ));
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Source("scan canceled".into()));
        }
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let overflow = Arc::new(AtomicBool::new(false));
        let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let flag = overflow.clone();
        let (out_send, out_recv) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = out_send.send(reader(out, flag));
        });
        let flag = overflow.clone();
        let (err_send, err_recv) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = err_send.send(reader(err, flag));
        });
        let start = Instant::now();
        let mut interrupted = None;
        let status = loop {
            if self.cancel.load(Ordering::Relaxed)
                || start.elapsed() > self.timeout
                || overflow.load(Ordering::Relaxed)
            {
                interrupted = Some("scan canceled, timed out, or exceeded the 8 MiB output budget");
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    let taskkill = std::env::var_os("SystemRoot")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| "C:/Windows".into())
                        .join("System32/taskkill.exe");
                    let _ = Command::new(taskkill)
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .creation_flags(0x08000000)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                let _ = child.kill();
                break child.wait()?;
            }
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(40));
        };
        let stdout = out_recv
            .recv_timeout(
                self.timeout
                    .saturating_sub(start.elapsed())
                    .max(Duration::from_millis(100)),
            )
            .map_err(|_| Error::Source("stdout did not close within the scan deadline".into()))??;
        let stderr = err_recv
            .recv_timeout(
                self.timeout
                    .saturating_sub(start.elapsed())
                    .max(Duration::from_millis(100)),
            )
            .map_err(|_| Error::Source("stderr did not close within the scan deadline".into()))??;
        if let Some(reason) = interrupted {
            return Err(Error::Source(reason.into()));
        }
        if overflow.load(Ordering::Relaxed) {
            return Err(Error::Source("inventory output exceeded budget".into()));
        }
        let decode = |b: Vec<u8>| {
            String::from_utf8(b).map_err(|_| Error::Source("inventory output was not UTF-8".into()))
        };
        Ok(CommandOutput {
            code: status.code().unwrap_or(-1),
            stdout: decode(stdout)?,
            stderr: decode(stderr)?,
        })
    }
}

/// Scripts are built only from this enum and constant argument lists, never package names.
pub fn script(manager: Manager, updates: bool) -> String {
    let prefix = "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; $global:LASTEXITCODE=0; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $OutputEncoding=[Console]::OutputEncoding; ";
    let body = match manager {
        Manager::Npm | Manager::Pnpm => {
            let exe = if manager == Manager::Npm {
                "npm.cmd"
            } else {
                "pnpm.cmd"
            };
            let outdated = if updates {
                format!(
                    "$u=& {exe} outdated -g --json; $ue=$LASTEXITCODE; if ($ue -notin @(0,1)) {{throw 'outdated command failed'}}; if ($u) {{$updates=($u|Out-String)}}"
                )
            } else {
                String::new()
            };
            let metadata = include_str!("inventory/npm-metadata.ps1");
            format!(
                "$root=& {exe} root -g; if ($LASTEXITCODE -ne 0) {{throw 'global root query failed'}}; $root=($root|Out-String).Trim(); $i=& {exe} list -g --depth=0 --json; $ie=$LASTEXITCODE; if ($ie -notin @(0,1)) {{throw 'installed listing failed'}}; $installed=($i|Out-String); {metadata} $updates=$null; $ue=$null; {outdated}; @{{root=$root;installedJson=$installed;updatesJson=$updates;installedExit=$ie;updateExit=$ue;metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 30 -Compress"
            )
        }
        Manager::Scoop => {
            let status = if updates {
                "$s=@(scoop status -l 6>$null); if ($LASTEXITCODE -ne 0) {throw 'Scoop status failed'}; $u=@($s|Select-Object Name,'Installed Version','Latest Version','Missing Dependencies',Info);"
            } else {
                "$u=$null;"
            };
            let metadata = include_str!("inventory/scoop-metadata.ps1");
            format!(
                "$i=scoop export; if ($LASTEXITCODE -ne 0) {{throw 'Scoop export failed'}}; $installed=($i|Out-String|ConvertFrom-Json); {metadata} {status} @{{installed=$installed;updates=$u;metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 20 -Compress"
            )
        }
        Manager::Chocolatey => {
            let outdated = if updates {
                "$u=& choco.exe outdated --limit-output --no-color; if ($LASTEXITCODE -notin @(0,2)) {throw 'Chocolatey outdated failed'};"
            } else {
                "$u=$null;"
            };
            let metadata = include_str!("inventory/chocolatey-metadata.ps1");
            format!(
                "$v=& choco.exe --version; if ($LASTEXITCODE -ne 0) {{throw 'Chocolatey version probe failed'}}; $a=@('list','--limit-output','--no-color'); if ([int](($v|Select-Object -Last 1).Split('.')[0]) -lt 2) {{$a+='--local-only'}}; $i=& choco.exe @a; if ($LASTEXITCODE -notin @(0,2)) {{throw 'Chocolatey listing failed'}}; {metadata} {outdated} @{{installed=($i|Out-String);updates=($u|Out-String);metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 8 -Compress"
            )
        }
        Manager::Winget => {
            let outdated = if updates {
                "$u=& winget.exe list --upgrade-available --disable-interactivity; $ue=$LASTEXITCODE;"
            } else {
                "$u=$null;$ue=$null;"
            };
            // Export has a stable JSON schema. Unknown/unmatched apps can be omitted by WinGet.
            let metadata = format!(
                "{}\n{}\n{}",
                include_str!("inventory/windows-executable.ps1"),
                include_str!("inventory/windows-registration.ps1"),
                include_str!("inventory/winget-metadata.ps1")
            );
            format!(
                "$tmp=[IO.Path]::GetTempFileName(); try {{ $null=& winget.exe export --output $tmp --include-versions --disable-interactivity; if ($LASTEXITCODE -ne 0) {{throw 'WinGet export failed; source agreements or matching may need attention'}}; $i=Get-Content -LiteralPath $tmp -Raw|ConvertFrom-Json; {metadata} {outdated} @{{installed=$i;updates=($u|Out-String);updateExit=$ue;metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 20 -Compress }} finally {{Remove-Item -LiteralPath $tmp -Force}}"
            )
        }
        Manager::Store => format!(
            "{}\n{}\n{}\n{}",
            include_str!("inventory/windows-executable.ps1"),
            include_str!("inventory/windows-registration.ps1"),
            include_str!("inventory/appx-presentation.ps1"),
            include_str!("inventory/store.ps1")
        ),
        Manager::Local => format!(
            "{}\n{}\n{}",
            include_str!("inventory/windows-registration.ps1"),
            include_str!("inventory/msi-fonts.ps1"),
            include_str!("inventory/local.ps1")
        ),
        Manager::VscodeInsiders | Manager::Vscode | Manager::Cursor => format!(
            "{}\n{}\n{}\n{}",
            match manager {
                Manager::VscodeInsiders =>
                    "$editorCommand='code-insiders.cmd';$editorDataName='Code - Insiders';$editorExtensions='.vscode-insiders';$editorApplication='code-insiders';$editorFolder='Microsoft VS Code Insiders';$editorExe='Code - Insiders.exe';$editorDisplayNames=@('Microsoft Visual Studio Code Insiders','Microsoft VS Code Insiders');$editorPublisher='Microsoft Corporation';",
                Manager::Vscode =>
                    "$editorCommand='code.cmd';$editorDataName='Code';$editorExtensions='.vscode';$editorApplication='code';$editorFolder='Microsoft VS Code';$editorExe='Code.exe';$editorDisplayNames=@('Microsoft Visual Studio Code','Microsoft VS Code');$editorPublisher='Microsoft Corporation';",
                Manager::Cursor =>
                    "$editorCommand='cursor.cmd';$editorDataName='Cursor';$editorExtensions='.cursor';$editorApplication='cursor';$editorFolder='Cursor';$editorExe='Cursor.exe';$editorDisplayNames=@('Cursor');$editorPublisher='Anysphere';",
                _ => unreachable!(),
            },
            include_str!("inventory/windows-registration.ps1"),
            include_str!("inventory/extension-icons.ps1"),
            include_str!("inventory/insiders.ps1")
        ),
        Manager::Bun => {
            let outdated = if updates {
                "$r=Read-Bun 'outdated --global';$u=$r.text;$ue=$r.code;"
            } else {
                "$u=$null;$ue=$null;"
            };
            let helper = "function Read-Bun($arguments) {$psi=[Diagnostics.ProcessStartInfo]::new();$psi.FileName=(Get-Command bun.exe -ErrorAction Stop).Source;$psi.Arguments=$arguments;$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardOutput=$true;$psi.RedirectStandardError=$true;$psi.StandardOutputEncoding=[Text.Encoding]::UTF8;$psi.StandardErrorEncoding=[Text.Encoding]::UTF8;$p=[Diagnostics.Process]::Start($psi);try {$a=$p.StandardOutput.ReadToEndAsync();$b=$p.StandardError.ReadToEndAsync();$p.WaitForExit();return @{text=($a.Result+\"`n\"+$b.Result);code=$p.ExitCode}} finally {$p.Dispose()}};";
            let inspect = include_str!("inventory/npm-metadata.ps1");
            let metadata = format!(
                "$metadata=@{{}};$metadataErrors=0;$base=($i -split \"`n\"|Where-Object {{$_ -match ' node_modules \\('}}|Select-Object -First 1) -replace ' node_modules \\(.*$','';if($base){{$path=Join-Path $base.Trim() 'package.json';if(Test-Path -LiteralPath $path){{$root=Join-Path $base.Trim() 'node_modules';$installed=Get-Content -LiteralPath $path -Raw;{inspect}}}}}"
            );
            format!(
                "$root=''; {helper} $r=Read-Bun 'pm ls -g';$i=$r.text;if ($r.code -ne 0) {{throw ('Bun global listing failed: '+$i)}};{metadata};$u='';$ue=0;if ($i -notmatch '\\(0 installed\\)') {{{outdated}}}; @{{root=$root;installed=$i;updates=$u;updateExit=$ue;metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 8 -Compress"
            )
        }
    };
    format!("{prefix}{body}")
}

pub fn scan(manager: Manager, updates: bool, runner: &impl Runner) -> Result<Scan> {
    let output = runner.run(&script(manager, updates))?;
    if output.code != 0 {
        return Err(Error::Source(format!(
            "{} inventory failed (exit {}): {}",
            manager.id(),
            output.code,
            output.stderr.chars().take(1000).collect::<String>()
        )));
    }
    parse(manager, updates, &output.stdout)
}
fn value<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && !matches!(*s, "Unknown" | "unknown" | "N/A" | "Not installed"))
}
fn items(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![v],
        _ => vec![],
    }
}
fn row(
    manager: Manager,
    id: &str,
    version: Option<&str>,
    source: &str,
    scope: &str,
    root: Option<&str>,
) -> InstalledPackage {
    InstalledPackage {
        manager,
        id: id.into(),
        name: id.into(),
        installed: version.map(str::to_string),
        available: None,
        candidate_check_succeeded: None,
        candidate_note: None,
        icon_path: None,
        icon_url: None,
        homepage: None,
        repository: None,
        repository_directory: None,
        release_notes: None,
        download_urls: vec![],
        package_root: None,
        extension: None,
        evidence: InventoryEvidence::default(),
        source: source.into(),
        scope: scope.into(),
        root: root.map(str::to_string),
        pinned: false,
        install_origin_known: !matches!(manager, Manager::Winget | Manager::Store | Manager::Local),
    }
}
fn attach_metadata(p: &mut InstalledPackage, metadata: &Value) {
    p.evidence.font_only = metadata["fontOnly"].as_bool();
    p.evidence.registration_id = value(metadata, "registrationId").map(str::to_owned);
    p.icon_path = value(metadata, "iconPath").map(str::to_owned);
    p.icon_url = value(metadata, "iconUrl").map(str::to_owned);
    p.homepage = value(metadata, "homepage").map(str::to_owned);
    p.repository = value(metadata, "repository").map(str::to_owned);
    p.repository_directory = value(metadata, "repositoryDirectory").map(str::to_owned);
    p.release_notes = value(metadata, "releaseNotes").map(str::to_owned);
    p.download_urls = metadata["downloads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(8)
        .map(str::to_owned)
        .collect();
    p.package_root = value(metadata, "packageRoot").map(str::to_owned);
    p.extension = metadata
        .get("extension")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    p.evidence.publisher = value(metadata, "publisher").map(str::to_owned);
    p.evidence.description = value(metadata, "description").map(str::to_owned);
    p.evidence.system_component = metadata["systemComponent"].as_bool();
    p.evidence.parent_package = value(metadata, "parentPackage").map(str::to_owned);
    p.evidence.release_type = value(metadata, "releaseType").map(str::to_owned);
    p.evidence.has_commands = metadata["hasCommands"].as_bool();
    p.evidence.has_shortcuts = metadata["hasShortcuts"].as_bool();
    p.evidence.runtime_only = metadata["runtimeOnly"].as_bool();
    p.evidence.library_only = metadata["libraryOnly"].as_bool();
    p.evidence.background_only = metadata["backgroundOnly"].as_bool();
    p.evidence.support_data = metadata["supportData"].as_bool();
    p.evidence.is_dependency = metadata["isDependency"].as_bool();
}
fn candidate_failure(scan: &mut Scan, id: &str, warning: String) {
    for record in scan.packages.iter_mut().filter(|p| p.id == id) {
        record.candidate_check_succeeded = Some(false);
        record.candidate_note = Some(warning.clone());
        record.available = None;
    }
    scan.warnings.push(warning);
    scan.update_check_succeeded = false;
}

fn apply_updates(scan: &mut Scan, updates: Vec<(String, String, String)>) {
    for (id, from, to) in updates {
        let matches: Vec<_> = scan
            .packages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.id == id && p.installed.as_deref() == Some(&from))
            .map(|(i, _)| i)
            .collect();
        if matches.len() == 1 {
            let record = &mut scan.packages[matches[0]];
            if record.candidate_check_succeeded != Some(false) {
                record.available = Some(to);
                record.candidate_check_succeeded = Some(true);
            }
        } else {
            candidate_failure(
                scan,
                &id,
                format!("ambiguous or stale update identity: {id}"),
            );
        }
    }
}
pub fn parse(manager: Manager, updates: bool, input: &str) -> Result<Scan> {
    let mut data: Value = serde_json::from_str(input.trim_start_matches('\u{feff}'))?;
    if matches!(manager, Manager::Npm | Manager::Pnpm) {
        for (raw, field) in [("installedJson", "installed"), ("updatesJson", "updates")] {
            if let Some(json) = data.get(raw).and_then(Value::as_str) {
                let parsed = if json.trim().is_empty() {
                    Value::Null
                } else {
                    serde_json::from_str(json)?
                };
                data[field] = parsed;
            }
        }
    }
    let mut scan = Scan {
        manager,
        checked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        candidate_checked_at: None,
        update_check_requested: updates,
        update_check_succeeded: updates,
        packages: vec![],
        warnings: vec![],
    };
    let mut candidates = vec![];
    match manager {
        Manager::Npm | Manager::Pnpm => {
            let root =
                value(&data, "root").ok_or_else(|| Error::Source("global root missing".into()))?;
            for group in items(&data["installed"]) {
                if group.get("error").is_some() {
                    return Err(Error::Source("manager returned a JSON error".into()));
                }
                if group.get("dependencies").is_none()
                    && (group.get("name").is_some() || group.get("path").is_some())
                {
                    continue;
                }
                let deps = group
                    .get("dependencies")
                    .and_then(Value::as_object)
                    .ok_or_else(|| Error::Source("dependencies object missing".into()))?;
                for (id, p) in deps {
                    let mut item = row(
                        manager,
                        id,
                        value(p, "version"),
                        "configured registry/source",
                        "global",
                        Some(root),
                    );
                    attach_metadata(&mut item, &data["metadata"][id]);
                    scan.packages.push(item);
                }
            }
            if data["installedExit"] != 0 {
                scan.warnings
                    .push("installed listing reported dependency problems".into());
            }
            if updates {
                // A valid outdated response covers each installed row independently.
                // Unmatched/malformed records below must not poison unrelated packages.
                for p in &mut scan.packages {
                    p.candidate_check_succeeded = Some(true);
                }
                if data["updateExit"]
                    .as_i64()
                    .is_none_or(|c| !matches!(c, 0 | 1))
                    || (data["updateExit"] == 1 && data["updates"].is_null())
                {
                    scan.update_check_succeeded = false;
                    scan.warnings
                        .push("outdated did not return a successful structured response".into());
                    for p in &mut scan.packages {
                        p.candidate_check_succeeded = Some(false);
                    }
                }
                if data["updates"].get("error").is_some() {
                    scan.update_check_succeeded = false;
                    scan.warnings.push("outdated returned a JSON error".into());
                    for p in &mut scan.packages {
                        p.candidate_check_succeeded = Some(false);
                    }
                } else if let Some(map) = data["updates"].as_object() {
                    for (id, u) in map {
                        if let (Some(from), Some(to)) = (value(u, "current"), value(u, "latest")) {
                            candidates.push((id.clone(), from.into(), to.into()));
                        } else {
                            candidate_failure(
                                &mut scan,
                                id,
                                format!("unrecognized update record: {id}"),
                            );
                        }
                    }
                } else if !data["updates"].is_null() {
                    return Err(Error::Source("unexpected outdated JSON shape".into()));
                }
            }
        }
        Manager::Scoop => {
            let apps = data["installed"]["apps"]
                .as_array()
                .ok_or_else(|| Error::Source("Scoop export apps array missing".into()))?;
            for app in apps {
                let id = value(app, "Name")
                    .ok_or_else(|| Error::Source("Scoop app name missing".into()))?;
                let info = value(app, "Info").unwrap_or("");
                let mut p = row(
                    manager,
                    id,
                    value(app, "Version"),
                    value(app, "Source").unwrap_or("unknown"),
                    if info.contains("Global install") {
                        "machine"
                    } else {
                        "user"
                    },
                    None,
                );
                p.pinned = info.contains("Held package");
                let metadata = &data["metadata"][id];
                attach_metadata(&mut p, metadata);
                p.root = value(metadata, "root").map(str::to_owned);
                if info.contains("Install failed") {
                    p.installed = None;
                    scan.warnings.push(format!("installation failed: {id}"));
                }
                scan.packages.push(p);
            }
            if updates {
                scan.warnings.push(
                    "Scoop candidates use local bucket manifests; buckets were not refreshed"
                        .into(),
                );
                let Some(statuses) = data["updates"].as_array() else {
                    return Err(Error::Source("Scoop status array missing".into()));
                };
                for p in &mut scan.packages {
                    let status = statuses.iter().find(|u| value(u, "Name") == Some(&p.id));
                    p.candidate_note = Some(match status {
                        None if p.installed.is_some() => "No newer version in local buckets".into(),
                        Some(u) if value(u, "Latest Version").is_some()
                            && value(u, "Installed Version") == p.installed.as_deref() =>
                            value(u, "Info").or_else(||value(u, "Missing Dependencies"))
                                .unwrap_or("Scoop candidate from local buckets").into(),
                        Some(u) => format!(
                            "Scoop status: {}",
                            value(u, "Info")
                                .or_else(|| value(u, "Missing Dependencies"))
                                .unwrap_or("candidate unavailable or identity ambiguous")
                        ),
                        _ => "Installed version unknown".into(),
                    });
                }
                for u in statuses {
                    if let (Some(id), Some(from), Some(to)) = (
                        value(u, "Name"),
                        value(u, "Installed Version"),
                        value(u, "Latest Version"),
                    ) {
                        candidates.push((id.into(), from.into(), to.into()));
                    }
                }
            }
        }
        Manager::Chocolatey => {
            for line in value(&data, "installed")
                .unwrap_or("")
                .lines()
                .filter(|l| !l.trim().is_empty())
            {
                let parts: Vec<_> = line.trim().split('|').collect();
                if parts.len() != 2 {
                    return Err(Error::Source(
                        "unexpected Chocolatey limited-output line".into(),
                    ));
                }
                let mut item = row(
                    manager,
                    parts[0],
                    Some(parts[1]),
                    "source not retained",
                    "unknown",
                    None,
                );
                let metadata = &data["metadata"][parts[0]];
                item.evidence.package_tags = value(metadata, "tags").unwrap_or("").into();
                item.evidence.wrapper_target = value(metadata, "wrapperTarget").map(str::to_owned);
                attach_metadata(&mut item, metadata);
                scan.packages.push(item);
            }
            if updates {
                for line in value(&data, "updates")
                    .unwrap_or("")
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                {
                    let parts: Vec<_> = line.trim().split('|').collect();
                    if parts.len() != 4 {
                        return Err(Error::Source("unexpected Chocolatey outdated line".into()));
                    }
                    candidates.push((parts[0].into(), parts[1].into(), parts[2].into()));
                }
            }
        }
        Manager::Winget => {
            let sources = data["installed"]["Sources"]
                .as_array()
                .ok_or_else(|| Error::Source("WinGet export Sources missing".into()))?;
            for source in sources {
                let name = value(&source["SourceDetails"], "Name").unwrap_or("unknown");
                let packages = source["Packages"]
                    .as_array()
                    .ok_or_else(|| Error::Source("WinGet Packages missing".into()))?;
                for p in packages {
                    let id = value(p, "PackageIdentifier")
                        .ok_or_else(|| Error::Source("WinGet package ID missing".into()))?;
                    let mut item = row(manager, id, value(p, "Version"), name, "unknown", None);
                    let metadata = &data["metadata"][format!("{name}/{id}")];
                    item.name = value(metadata, "name").unwrap_or(id).into();
                    attach_metadata(&mut item, metadata);
                    scan.packages.push(item);
                }
            }
            scan.warnings.push("WinGet export omits unmatched apps; the local provider covers uninstall registrations separately. Detection does not establish the original installer or scope".into());
            if updates {
                if data["updateExit"] != 0 {
                    scan.update_check_succeeded = false;
                    scan.warnings
                        .push("WinGet candidate check failed or returned no applications".into());
                } else {
                    match winget_updates(value(&data, "updates").unwrap_or("")) {
                        Ok(rows) => candidates = rows,
                        Err(e) => {
                            scan.update_check_succeeded = false;
                            scan.warnings.push(e.to_string());
                        }
                    }
                }
            }
        }
        Manager::Store => {
            if !data.is_array() && !data.is_null() {
                return Err(Error::Source("Appx array missing".into()));
            }
            for p in items(&data) {
                let id = value(p, "PackageFamilyName")
                    .ok_or_else(|| Error::Source("Appx family identity missing".into()))?;
                let mut item = row(
                    manager,
                    id,
                    value(p, "Version"),
                    "Appx registration",
                    "current user",
                    value(p, "InstallLocation"),
                );
                item.name = value(p, "Name").unwrap_or(id).into();
                item.evidence.non_removable = p.get("NonRemovable").and_then(Value::as_bool);
                item.evidence.signature_kind = value(p, "SignatureKind").map(str::to_owned);
                item.evidence.has_visible_app = p.get("HasVisibleApp").and_then(Value::as_bool);
                item.evidence.has_execution_alias =
                    p.get("HasExecutionAlias").and_then(Value::as_bool);
                item.evidence.has_cli_alias = p["HasCliAlias"].as_bool();
                item.evidence.is_development_mode = p["IsDevelopmentMode"].as_bool();
                item.evidence.publisher = value(p, "Publisher").map(str::to_owned);
                item.evidence.is_inbox = p["IsInbox"].as_bool();
                item.evidence.has_preinstalled_task = p["HasPreinstalledTask"].as_bool();
                item.name = value(p, "FriendlyName").unwrap_or(&item.name).to_owned();
                item.icon_path = value(p, "IconPath").map(str::to_owned);
                scan.packages.push(item);
            }
            scan.update_check_succeeded = false;
            scan.warnings.push("Current-user Appx registrations include side-loaded apps; Store origin and available versions are not inferred".into());
        }
        Manager::Local => {
            let installed = data["installed"]
                .as_array()
                .ok_or_else(|| Error::Source("local installed array missing".into()))?;
            let mut seen = std::collections::HashSet::new();
            for p in installed {
                let id = value(p, "Id")
                    .ok_or_else(|| Error::Source("local registration identity missing".into()))?;
                if !seen.insert(id.to_ascii_lowercase()) {
                    return Err(Error::Source(
                        "duplicate local registration identity".into(),
                    ));
                }
                let name = value(p, "Name")
                    .ok_or_else(|| Error::Source("local display name missing".into()))?;
                let scope =
                    value(p, "Scope").ok_or_else(|| Error::Source("local scope missing".into()))?;
                let mut item = row(
                    manager,
                    id,
                    value(p, "Version"),
                    "Windows uninstall registration",
                    scope,
                    value(p, "InstallLocation"),
                );
                attach_metadata(&mut item, &p["metadata"]);
                item.evidence.registration_id = Some(id.into());
                item.name = name.into();
                item.icon_path = value(p, "IconPath").map(str::to_owned);
                item.candidate_note =
                    Some("Local registration only; no update source is configured".into());
                scan.packages.push(item);
            }
            scan.update_check_succeeded = false;
            scan.warnings.push("Uninstall registrations establish local presence, not the original installer or update availability; unregistered portable apps are not discovered".into());
        }
        Manager::VscodeInsiders | Manager::Vscode | Manager::Cursor => {
            if data["editorCliUnavailable"] == true {
                return Err(Error::Source(
                    "Editor detected, but its extension-listing command is unavailable".into(),
                ));
            }
            let legacy = json_profile(&data);
            let profiles = match data.get("profiles") {
                Some(v) => v
                    .as_array()
                    .ok_or_else(|| Error::Source("Editor profiles array missing".into()))?,
                None => &legacy,
            };
            let pattern = regex::Regex::new(
                r"^([A-Za-z0-9][A-Za-z0-9_-]*\.[A-Za-z0-9][A-Za-z0-9_.-]*)@([^\s]+)$",
            )
            .unwrap();
            let mut seen = std::collections::HashSet::new();
            for profile in profiles {
                let name = value(profile, "name")
                    .ok_or_else(|| Error::Source("Editor profile name missing".into()))?;
                let id = value(profile, "id")
                    .ok_or_else(|| Error::Source("Editor profile identity missing".into()))?;
                if !seen.insert(id) {
                    return Err(Error::Source("Duplicate editor profile identity".into()));
                }
                let body = profile["installed"]
                    .as_str()
                    .ok_or_else(|| Error::Source("Editor extension listing missing".into()))?;
                for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let c = pattern.captures(line).ok_or_else(|| {
                        Error::Source("unexpected editor extension listing".into())
                    })?;
                    let mut item = row(
                        manager,
                        &c[1],
                        Some(&c[2]),
                        manager.editor_name().unwrap(),
                        &format!("profile: {name}"),
                        value(profile, "root"),
                    );
                    attach_metadata(&mut item, &profile["metadata"][&c[1]]);
                    let info = item.extension.get_or_insert_with(Default::default);
                    info.id = c[1].into();
                    info.profile_id = Some(id.into());
                    info.profile_name = Some(name.into());
                    info.profile_icon = value(profile, "icon")
                        .filter(|icon| {
                            !icon.is_empty() && icon.len() <= 96
                                && icon.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                        })
                        .map(str::to_owned);
                    item.source = format!("{} extensions", manager.editor_name().unwrap());
                    scan.packages.push(item);
                }
            }
            scan.update_check_succeeded = false;
            if !profiles.is_empty() {
                scan.warnings.push("Registered local editor profiles are enumerated, including inherited extensions. Remote hosts are not scanned. Marketplace candidates are not yet checked (compatibility and prerelease channel must be preserved).".into());
            }
        }
        Manager::Bun => {
            let body = value(&data, "installed").unwrap_or("");
            let reported_root = body
                .lines()
                .find_map(|l| l.split_once(" node_modules (").map(|(root, _)| root.trim()));
            let root = reported_root
                .or_else(|| value(&data, "root"))
                .ok_or_else(|| Error::Source("Bun global root missing".into()))?;
            if data["missing"] == true {
                scan.update_check_succeeded = false;
                scan.warnings.push(
                    "Bun global package.json not found; no project folders were searched".into(),
                );
                return Ok(scan);
            }
            let re = regex::Regex::new(
                r"(?m)^\s*[\x{251c}\x{2514}]\x{2500}\x{2500}\s+(.+)@([^\s]+)\s*$",
            )
            .unwrap();
            for c in re.captures_iter(body) {
                let mut item = row(
                    manager,
                    &c[1],
                    Some(&c[2]),
                    "configured registry/source",
                    "global",
                    Some(root),
                );
                attach_metadata(&mut item, &data["metadata"][&c[1]]);
                scan.packages.push(item);
            }
            if scan.packages.is_empty() && !body.contains("(0)") && !body.contains("(0 installed)")
            {
                return Err(Error::Source("unrecognized Bun package tree".into()));
            }
            if updates {
                if data["updateExit"] != 0 && data["updateExit"] != 1 {
                    scan.update_check_succeeded = false;
                    scan.warnings.push("Bun outdated failed".into());
                } else {
                    candidates = bun_updates(value(&data, "updates").unwrap_or(""))?;
                }
            }
        }
    }
    if data["metadataErrors"].as_u64().unwrap_or(0) > 0 {
        scan.warnings.push(
            "Some local classification metadata was unavailable; uncertain entries remain visible"
                .into(),
        );
    }
    apply_updates(&mut scan, candidates);
    if scan.update_check_requested {
        for record in &mut scan.packages {
            record
                .candidate_check_succeeded
                .get_or_insert(scan.update_check_succeeded);
        }
    }
    if scan.update_check_requested
        && (scan.update_check_succeeded
            || scan
                .packages
                .iter()
                .any(|p| p.candidate_check_succeeded == Some(true)))
    {
        scan.candidate_checked_at = Some(scan.checked_at.clone());
    }
    Ok(scan)
}
fn json_profile(data: &Value) -> Vec<Value> {
    vec![
        serde_json::json!({"name":"Default","id":"__default__profile__","installed":data["installed"]}),
    ]
}
fn bun_updates(text: &str) -> Result<Vec<(String, String, String)>> {
    let strip = regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap();
    let clean = strip.replace_all(text, "").replace('\u{2502}', "|");
    let mut rows = vec![];
    for line in clean.lines().filter(|l| l.trim_start().starts_with('|')) {
        let parts: Vec<_> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if parts.first().is_some_and(|p| {
            p.eq_ignore_ascii_case("Package")
                || p.chars().all(|c| c == '-' || c == ':' || c.is_whitespace())
        }) {
            continue;
        }
        if parts.len() < 4 {
            return Err(Error::Source("unexpected Bun outdated columns".into()));
        }
        rows.push((
            parts[0].split_whitespace().next().unwrap_or("").into(),
            parts[1].into(),
            parts[3].into(),
        ));
    }
    if rows.is_empty()
        && !text.trim().is_empty()
        && !text.contains("up to date")
        && !text.contains("Package")
    {
        return Err(Error::Source("unrecognized Bun outdated output".into()));
    }
    Ok(rows)
}
fn winget_updates(text: &str) -> Result<Vec<(String, String, String)>> {
    let lines: Vec<_> = text.lines().collect();
    let (index, header) = lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.contains("Id") && l.contains("Version") && l.contains("Available"))
        .ok_or_else(|| {
            Error::Source(
                "WinGet update table layout/locale unsupported; installed export retained".into(),
            )
        })?;
    let starts = [
        header.find("Id").unwrap(),
        header.find("Version").unwrap(),
        header.find("Available").unwrap(),
    ];
    if !(starts[0] < starts[1] && starts[1] < starts[2]) {
        return Err(Error::Source("invalid WinGet columns".into()));
    }
    let end = header.find("Source");
    let mut rows = vec![];
    for line in lines.iter().skip(index + 2) {
        if !line.is_ascii() {
            return Err(Error::Source(
                "non-ASCII WinGet table needs a structured provider; candidates not inferred"
                    .into(),
            ));
        }
        if line.len() <= starts[2] {
            continue;
        }
        let id = line[starts[0]..starts[1]].trim();
        let from = line[starts[1]..starts[2]].trim();
        let to = line[starts[2]..end.unwrap_or(line.len()).min(line.len())].trim();
        if id.is_empty()
            || id.contains("...")
            || id.split_whitespace().count() != 1
            || from.is_empty()
            || to.is_empty()
        {
            return Err(Error::Source("ambiguous WinGet update row".into()));
        }
        rows.push((id.into(), from.into(), to.into()));
    }
    Ok(rows)
}
