use releasescout_engine::{
    inventory::{Manager, parse},
    presentation::{component_reason, observations},
};
use serde_json::json;

#[test]
fn system_policy_uses_registration_and_publisher_evidence_not_installed_app_names() {
    for (publisher, signature, cli, preinstalled, hidden) in [
        ("O=Microsoft Corporation", "Store", false, false, true),
        ("O=Microsoft Corporation", "System", false, false, true),
        ("O=Microsoft Corporation", "Developer", false, false, false),
        ("O=Microsoft Corporation", "Store", true, false, false),
        ("O=Unfamiliar Publisher", "Store", false, false, false),
        ("O=Unfamiliar Publisher", "Store", false, true, true),
        ("O=Unfamiliar Publisher", "Store", true, true, false),
    ] {
        let scan = parse(
            Manager::Store,
            false,
            &json!([{
                "Name":"Unseen.App","PackageFamilyName":"Unseen.App_randompublisher",
                "Version":"1.0","HasVisibleApp":true,"HasExecutionAlias":true,
                "Publisher":publisher,"SignatureKind":signature,"HasCliAlias":cli,
                "HasPreinstalledTask":preinstalled
            }])
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            component_reason(&scan.packages[0]).is_some(),
            hidden,
            "{publisher}/{signature}/cli={cli}/preinstalled={preinstalled}"
        );
    }
}

#[test]
fn unknown_metadata_is_not_a_reason_to_hide_an_app_or_cli() {
    for (metadata, hidden) in [
        (json!({}), false),
        (json!({"runtimeOnly":true}), true),
        (json!({"runtimeOnly":false,"hasCommands":true}), false),
        (json!({"libraryOnly":true}), true),
        (
            json!({"isDependency":true,"hasCommands":true,"hasShortcuts":false}),
            false,
        ),
        (
            json!({"isDependency":true,"hasCommands":false,"hasShortcuts":false}),
            true,
        ),
        (
            json!({"isDependency":true,"hasCommands":false,"hasShortcuts":true}),
            false,
        ),
    ] {
        let scan = parse(
            Manager::Scoop,
            false,
            &json!({"installed":{"apps":[
            {"Name":"never-seen-tool","Version":"1.0","Source":"new-bucket"}
        ]},"metadata":{"never-seen-tool":metadata}})
            .to_string(),
        )
        .unwrap();
        assert_eq!(component_reason(&scan.packages[0]).is_some(), hidden);
    }
}

#[test]
fn production_microsoft_packages_and_development_registrations_are_distinguished() {
    for (development, hidden) in [(Some(false), true), (Some(true), false), (None, false)] {
        let scan=parse(Manager::Store,false,&json!([{
            "Name":"Unseen.MicrosoftApp","PackageFamilyName":"Unseen.MicrosoftApp_pub","Version":"1.0",
            "Publisher":"O=Microsoft Corporation","SignatureKind":"Developer","HasCliAlias":false,
            "IsDevelopmentMode":development
        }]).to_string()).unwrap();
        assert_eq!(component_reason(&scan.packages[0]).is_some(), hidden);
    }
}

#[test]
fn global_cli_manifests_and_libraries_are_distinguished_independently_of_manager() {
    for manager in [Manager::Npm, Manager::Pnpm, Manager::Bun] {
        for (has_commands, hidden) in [(Some(true), false), (Some(false), true), (None, false)] {
            let mut scan = parse(Manager::Npm, false, &json!({"root":"Z:/another-machine/global",
                "installed":{"dependencies":{"@new-vendor/unseen":{"version":"1.0"}}},"installedExit":0,
                "metadata":{"@new-vendor/unseen":{"hasCommands":has_commands}}
            }).to_string()).unwrap();
            scan.packages[0].manager = manager;
            assert_eq!(component_reason(&scan.packages[0]).is_some(), hidden);
            scan.packages[0].scope = "project".into();
            assert!(component_reason(&scan.packages[0]).is_none());
        }
    }
}

#[test]
fn installer_roles_do_not_hide_sdks_because_their_tags_mention_runtime() {
    for (metadata, hidden) in [
        (json!({"tags":"vendor workload cpp"}), true),
        (json!({"tags":"vendor redistributable"}), true),
        (
            json!({"tags":"java runtime compiler sdk","hasCommands":true}),
            false,
        ),
        (json!({"systemComponent":true}), true),
        (json!({"parentPackage":"a-parent-installation"}), true),
        (json!({"releaseType":"Security Update"}), true),
        (json!({"tags":"runtime","wrapperTarget":"real-app"}), false),
    ] {
        let scan = parse(
            Manager::Chocolatey,
            false,
            &json!({"installed":"unseen-vendor-package|1.0",
            "metadata":{"unseen-vendor-package":metadata}})
            .to_string(),
        )
        .unwrap();
        assert_eq!(component_reason(&scan.packages[0]).is_some(), hidden);
    }
}

#[test]
fn duplicate_provider_records_cannot_resurrect_a_classified_system_app() {
    let store = parse(
        Manager::Store,
        false,
        &json!([{
            "Name":"Outlook","PackageFamilyName":"Microsoft.OutlookForWindows_8wekyb3d8bbwe",
            "Version":"1.0","HasPreinstalledTask":true,"HasCliAlias":false
        }])
        .to_string(),
    )
    .unwrap();
    let winget = parse(Manager::Winget,false,&json!({"installed":{"Sources":[
        {"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"Microsoft.Outlook","Version":"1.0"}]},
        {"SourceDetails":{"Name":"private"},"Packages":[{"PackageIdentifier":"Microsoft.Outlook","Version":"1.0"}]}
    ]}}).to_string()).unwrap();
    let groups = observations(&[store, winget]);
    assert!(groups[0].component_reason.is_some());
    assert_eq!(groups[0].component_reason, groups[1].component_reason);
    assert!(groups[2].component_reason.is_none());
}

#[test]
fn local_support_evidence_does_not_hide_a_separate_standalone_installation() {
    let scan = parse(
        Manager::Chocolatey,
        false,
        &json!({"installed":"python311|3.11\npython314|3.14",
            "metadata":{"python311":{"libraryOnly":true},"python314":{"hasCommands":true}}
        })
        .to_string(),
    )
    .unwrap();
    let groups = observations(&[scan]);
    assert_eq!(groups[0].group, groups[1].group);
    assert!(groups[0].component_reason.is_some());
    assert!(groups[1].component_reason.is_none());
}

#[test]
fn insiders_profiles_keep_membership_and_versions_but_group_the_same_extension() {
    let scan = parse(Manager::VscodeInsiders,false,&json!({"profiles":[
        {"id":"default","name":"Default","root":"D:/settings/User","installed":"vendor.shared@1.0\n"},
        {"id":"work","name":"Work & Review","root":"D:/settings/User/profiles/work","installed":"vendor.shared@2.0-pre.1\nvendor.other@3.0\n"},
            {"id":"inherited","name":"Inherited","root":"D:/settings/User/profiles/inherited","installed":"Vendor.Shared@1.0\n"}
    ]}).to_string()).unwrap();
    assert_eq!(scan.packages.len(), 4);
    assert_eq!(scan.packages[1].scope, "profile: Work & Review");
    assert_eq!(scan.packages[1].installed.as_deref(), Some("2.0-pre.1"));
    let groups = observations(std::slice::from_ref(&scan));
    assert_eq!(groups[0].group, groups[1].group);
    assert_eq!(groups[0].group, groups[3].group);
    assert_ne!(groups[0].group, groups[2].group);
    assert!(groups.iter().all(|g| g.component_reason.is_none()));
}

#[test]
fn malformed_or_duplicate_profile_results_are_not_silently_accepted_as_complete() {
    for profiles in [
        json!(null),
        json!([{"id":"a","name":"A"}]),
        json!([
            {"id":"same","name":"A","installed":""},{"id":"same","name":"B","installed":""}
        ]),
    ] {
        assert!(
            parse(
                Manager::VscodeInsiders,
                false,
                &json!({"profiles":profiles}).to_string()
            )
            .is_err()
        );
    }
}

#[cfg(windows)]
mod windows_metadata {
    use super::*;
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fn powershell(script: &str, env: &[(&str, &Path)]) -> String {
        let mut child=Command::new("powershell.exe").args(["-NoLogo","-NoProfile","-NonInteractive","-Command",
            &format!("$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);{script}")])
            .envs(env.iter().map(|(key,path)|(*key,*path)))
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let started = Instant::now();
        let mut timeout = false;
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > Duration::from_secs(15) {
                child.kill().unwrap();
                timeout = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            !timeout && output.status.success(),
            "timeout={timeout}: {} {}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    #[test]
    fn extension_icons_require_registered_membership_exact_version_and_confined_paths() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("custom extensions/vendor.tool");
        write(
            &app.join("package.json"),
            &json!({"publisher":"vendor","name":"tool","version":"2.0","icon":"media/logo.png","description":"Provider-authored description"})
                .to_string(),
        );
        write(
            &app.join("media/logo.png"),
            "fixture bytes; only metadata is read",
        );
        let registry = dir.path().join("profile/extensions.json");
        let uri = url::Url::from_directory_path(&app).unwrap();
        write(&registry,&json!([{"identifier":{"id":"vendor.tool"},"version":"2.0","location":{"scheme":"file","path":uri.path()}}]).to_string());
        let script = format!(
            "{};$m=Read-ExtensionIconMetadata $env:REGISTRY 'vendor.tool@2.0';$old=Read-ExtensionIconMetadata $env:REGISTRY 'vendor.tool@1.0';@{{found=$m;old=$old}}|ConvertTo-Json -Depth 10 -Compress",
            include_str!("../src/inventory/extension-icons.ps1")
        );
        let result: serde_json::Value =
            serde_json::from_str(&powershell(&script, &[("REGISTRY", &registry)])).unwrap();
        let icon = result["found"]["vendor.tool"]["iconPath"].as_str().unwrap();
        assert_eq!(
            fs::canonicalize(icon).unwrap(),
            fs::canonicalize(app.join("media/logo.png")).unwrap()
        );
        assert!(result["old"].as_object().unwrap().is_empty());
        assert_eq!(
            result["found"]["vendor.tool"]["description"],
            "Provider-authored description"
        );
        write(
            &app.join("package.json"),
            &json!({"publisher":"vendor","name":"tool","version":"2.0","icon":"../escape.png"})
                .to_string(),
        );
        let result: serde_json::Value =
            serde_json::from_str(&powershell(&script, &[("REGISTRY", &registry)])).unwrap();
        assert!(result["found"]["vendor.tool"]["iconPath"].is_null());
        assert!(result["found"]["vendor.tool"]["extension"].is_object());
    }

    #[test]
    fn extension_descriptions_resolve_localized_text_and_omit_unresolved_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("extension");
        write(
            &root.join("package.json"),
            &json!({
                "publisher":"vendor","name":"tool","version":"2.0",
                "description":"%extension.description%"
            })
            .to_string(),
        );
        let registry = dir.path().join("extensions.json");
        let uri = url::Url::from_directory_path(&root).unwrap();
        write(
            &registry,
            &json!([{"identifier":{"id":"vendor.tool"},"version":"2.0",
            "location":{"scheme":"file","path":uri.path()}}])
            .to_string(),
        );
        let script = format!(
            "{};Read-ExtensionIconMetadata $env:REGISTRY 'vendor.tool@2.0'|ConvertTo-Json -Depth 10 -Compress",
            include_str!("../src/inventory/extension-icons.ps1")
        );
        for (nls, expected) in [
            (
                r#"{"extension.description":"A localized provider description"}"#,
                Some("A localized provider description"),
            ),
            (r#"{"different.key":"Not this description"}"#, None),
            ("malformed JSON", None),
        ] {
            write(&root.join("package.nls.json"), nls);
            let result: serde_json::Value =
                serde_json::from_str(&powershell(&script, &[("REGISTRY", &registry)])).unwrap();
            assert_eq!(result["vendor.tool"]["description"].as_str(), expected);
            assert!(result["vendor.tool"]["extension"].is_object());
        }
    }
    #[test]
    fn relocated_scoop_manifests_distinguish_jre_sdk_cli_and_dependency_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("custom portable root");
        for (id, manifest) in [
            (
                "unfamiliar-cli",
                json!({"architecture":{"arm64":{"bin":"tool.exe"}},"depends":"unfamiliar-data"}),
            ),
            (
                "unfamiliar-data",
                json!({"description":"Datos","url":"https://example.test/data.pem"}),
            ),
            (
                "unfamiliar-runtime",
                json!({"env_set":{"JAVA_HOME":"$dir"},"env_add_path":"bin"}),
            ),
            (
                "unfamiliar-sdk",
                json!({"env_set":{"JAVA_HOME":"$dir"},"env_add_path":"bin"}),
            ),
        ] {
            let modern = id == "unfamiliar-cli";
            write(
                &root.join(format!(
                    "apps/{id}/current/{}",
                    if modern {
                        "scoop-manifest.json"
                    } else {
                        "manifest.json"
                    }
                )),
                &manifest.to_string(),
            );
            if modern {
                write(
                    &root.join(format!("apps/{id}/current/scoop-install.json")),
                    "{\"architecture\":\"arm64\"}",
                );
                write(
                    &root.join(format!("apps/{id}/current/install.json")),
                    "An app-owned file, not a Scoop receipt",
                );
            }
        }
        for id in ["unfamiliar-runtime", "unfamiliar-sdk"] {
            write(&root.join(format!("apps/{id}/current/bin/java.exe")), "");
        }
        write(&root.join("apps/unfamiliar-sdk/current/bin/javac.exe"), "");
        let apps = [
            "unfamiliar-cli",
            "unfamiliar-data",
            "unfamiliar-runtime",
            "unfamiliar-sdk",
        ]
        .map(|id| json!({"Name":id,"Version":"1.0","Source":"another-vendor"}));
        let export = dir.path().join("export.json");
        write(&export, &json!({"apps":apps}).to_string());
        let result = powershell(
            &format!(
                "$installed=Get-Content -LiteralPath $env:FIXTURE -Raw|ConvertFrom-Json;{};@{{installed=$installed;metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 20 -Compress",
                include_str!("../src/inventory/scoop-metadata.ps1")
            ),
            &[("SCOOP", &root), ("FIXTURE", &export)],
        );
        let scan = parse(Manager::Scoop, false, &result).unwrap();
        assert_eq!(scan.packages[0].evidence.has_commands, Some(true));
        assert_eq!(scan.packages[1].evidence.support_data, Some(true));
        assert!(component_reason(&scan.packages[0]).is_none());
        assert!(component_reason(&scan.packages[1]).is_some());
        assert!(component_reason(&scan.packages[2]).is_some());
        assert!(component_reason(&scan.packages[3]).is_none());
        assert!(scan.warnings.is_empty());
    }
    #[test]
    fn pnpm_isolated_package_paths_and_legacy_bin_directories_are_supported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("global root");
        let command = dir.path().join("isolated cli/node_modules/@future/tool");
        let library = dir.path().join("isolated library/node_modules/library");
        write(
            &command.join("package.json"),
            &json!({"name":"@future/tool","version":"1.0","directories":{"bin":"commands"}})
                .to_string(),
        );
        write(
            &library.join("package.json"),
            &json!({"name":"library","version":"1.0"}).to_string(),
        );
        let listing = dir.path().join("listing.json");
        write(&listing,&json!([{"dependencies":{
            "@future/tool":{"version":"1.0","path":command},"library":{"version":"1.0","path":library}
        }}]).to_string());
        let script = format!(
            "$root=$env:FIXTURE_ROOT;$installed=[IO.File]::ReadAllText($env:FIXTURE_LIST);{};@{{root=$root;installedJson=$installed;installedExit=0;metadata=$metadata}}|ConvertTo-Json -Depth 20 -Compress",
            include_str!("../src/inventory/npm-metadata.ps1")
        );
        let scan = parse(
            Manager::Pnpm,
            false,
            &powershell(
                &script,
                &[("FIXTURE_ROOT", &root), ("FIXTURE_LIST", &listing)],
            ),
        )
        .unwrap();
        assert!(component_reason(&scan.packages[0]).is_none());
        assert!(component_reason(&scan.packages[1]).is_some());
    }

    #[test]
    fn vscode_inventory_distinguishes_an_uninstalled_editor_from_a_missing_cli() {
        let dir = tempfile::tempdir().unwrap();
        let roaming = dir.path().join("roaming");
        let local = dir.path().join("local");
        let programs = dir.path().join("programs");
        let absent = dir.path().join("absent");
        let data = roaming.join("Code");
        write(&data.join("User/globalStorage/storage.json"), "{}");
        let scan_script = releasescout_engine::inventory::script(Manager::Vscode, false)
            .replacen("function Get-LocalRegistrations {", "function Get-RealLocalRegistrations {", 1);
        let mock = "function Get-Command {param($Name,$CommandType,[switch]$All,$ErrorAction)};";
        let env = [
            ("APPDATA", roaming.as_path()),
            ("LOCALAPPDATA", local.as_path()),
            ("ProgramFiles", programs.as_path()),
            ("ProgramFiles(x86)", programs.as_path()),
            ("ProgramW6432", programs.as_path()),
            ("VSCODE_PORTABLE", absent.as_path()),
            ("RELEASESCOUT_VSCODE_USER_DATA", data.as_path()),
        ];
        let scan = |registration: &str| {
            parse(
                Manager::Vscode,
                false,
                &powershell(&format!("{mock}{registration};{scan_script}"), &env),
            )
        };
        let insiders = "function Get-LocalRegistrations {[pscustomobject]@{DisplayName='Microsoft Visual Studio Code Insiders (User)';Publisher='Microsoft Corporation'}}";
        let removed = scan(insiders).unwrap();
        assert!(removed.packages.is_empty());
        assert!(removed.warnings.is_empty());
        assert!(data.join("User/globalStorage/storage.json").is_file());

        let stable = "function Get-LocalRegistrations {[pscustomobject]@{DisplayName='Microsoft Visual Studio Code (User)';Publisher='Microsoft Corporation'}}";
        assert!(scan(stable).unwrap_err().to_string().contains("Editor detected"));

        let app = local.join("Programs/Microsoft VS Code");
        write(&app.join("Code.exe"), "");
        write(&app.join("bin/code.cmd"), "@echo off\r\necho vendor.tool@1.0\r\n");
        write(
            &app.join("resources/app/product.json"),
            &json!({"applicationName":"code","nameShort":"Code"}).to_string(),
        );
        assert_eq!(scan(stable).unwrap().packages[0].id, "vendor.tool");
    }

    #[test]
    fn native_insiders_bootstrap_preserves_profile_names_and_paths_as_data() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("editor");
        let cli = app.join("bin/code-insiders.cmd");
        write(&cli, "@echo off\r\nexit /b 99\r\n");
        write(
            &app.join("build/resources/app/product.json"),
            &json!({"quality":"insider","nameShort":"Code - Insiders"}).to_string(),
        );
        write(&app.join("build/resources/app/out/cli.js"), "");
        let roaming = dir.path().join("roaming % & custom");
        let data = roaming.join("Code - Insiders");
        let profile_name = "--install-extension=not-a-command & \"quoted\"";
        write(
            &data.join("User/globalStorage/storage.json"),
            &json!({"userDataProfiles":[{"name":profile_name,"location":"work"}]}).to_string(),
        );
        let source = dir.path().join("mock.cs");
        write(
            &source,
            r#"using System;using System.IO;class Mock {static int Main(string[] args) {
            string u=Array.Find(args,a=>a.StartsWith("--user-data-dir="));if(u==null||!Directory.Exists(u.Substring("--user-data-dir=".Length)))return 8;
            string p=Array.Find(args,a=>a.StartsWith("--profile="));if(p==null){Console.WriteLine("vendor.shared@1.0");return 0;}
            if(p.Substring("--profile=".Length)!="--install-extension=not-a-command & \"quoted\"")return 9;
            Console.WriteLine("vendor.work@2.0");return 0;}}"#,
        );
        let exe = app.join("Code - Insiders.exe");
        let script = format!(
            "Add-Type -TypeDefinition ([IO.File]::ReadAllText($env:FIXTURE_SOURCE)) -OutputAssembly $env:FIXTURE_EXE -OutputType ConsoleApplication;function Get-Command {{param($Name,$ErrorAction) [pscustomobject]@{{Source=$env:FIXTURE_CLI}} }};{}",
            include_str!("../src/inventory/insiders.ps1")
        );
        let result = powershell(
            &script,
            &[
                ("FIXTURE_CLI", &cli),
                ("FIXTURE_SOURCE", &source),
                ("FIXTURE_EXE", &exe),
                ("APPDATA", &roaming),
                ("RELEASESCOUT_INSIDERS_USER_DATA", &data),
                ("VSCODE_PORTABLE", &dir.path().join("absent")),
            ],
        );
        let scan = parse(Manager::VscodeInsiders, false, &result).unwrap();
        assert_eq!(scan.packages.len(), 2);
        assert_eq!(scan.packages[1].scope, format!("profile: {profile_name}"));
        assert_eq!(scan.packages[1].id, "vendor.work");
    }

    #[test]
    fn console_capabilities_come_from_pe_headers_not_alias_names() {
        let dir = tempfile::tempdir().unwrap();
        for (name, magic, subsystem) in [
            ("arbitrary-console.exe", 0x10bu16, 3u16),
            ("arbitrary-gui.exe", 0x20b, 2),
        ] {
            let mut bytes = vec![0; 256];
            bytes[0..2].copy_from_slice(&0x5a4du16.to_le_bytes());
            bytes[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
            bytes[64..68].copy_from_slice(&0x4550u32.to_le_bytes());
            bytes[84..86].copy_from_slice(&80u16.to_le_bytes());
            bytes[88..90].copy_from_slice(&magic.to_le_bytes());
            bytes[156..158].copy_from_slice(&subsystem.to_le_bytes());
            fs::write(dir.path().join(name), bytes).unwrap();
        }
        fs::write(dir.path().join("invalid.exe"), [0; 16]).unwrap();
        let script = format!(
            "{};@{{console=(Test-ConsoleExecutable (Join-Path $env:FIXTURE_ROOT 'arbitrary-console.exe'));gui=(Test-ConsoleExecutable (Join-Path $env:FIXTURE_ROOT 'arbitrary-gui.exe'));unknown=(Test-ConsoleExecutable (Join-Path $env:FIXTURE_ROOT 'invalid.exe'))}}|ConvertTo-Json -Compress",
            include_str!("../src/inventory/windows-executable.ps1")
        );
        let result: serde_json::Value =
            serde_json::from_str(&powershell(&script, &[("FIXTURE_ROOT", dir.path())])).unwrap();
        assert_eq!(result["console"], true);
        assert_eq!(result["gui"], false);
        assert!(result["unknown"].is_null());
    }

    #[test]
    fn library_payload_detection_reads_cdata_and_excludes_only_the_recorded_uninstaller() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("package manager");
        let payload = dir.path().join("some library");
        write(
            &root.join("lib/unfamiliar-addon/unfamiliar-addon.nuspec"),
            "<package><metadata><description>Addon</description></metadata></package>",
        );
        write(&payload.join("vendor.dll"), "");
        write(&payload.join("remove.exe"), "");
        let receipt = format!(
            "<registrySnapshot><keys><key><InstallLocation><![CDATA[{}]]></InstallLocation><UninstallString><![CDATA[\"{}\" /quiet]]></UninstallString></key></keys></registrySnapshot>",
            payload.display(),
            payload.join("../some library/remove.exe").display()
        );
        write(
            &root.join(".chocolatey/unfamiliar-addon.1.0/.registry"),
            &receipt,
        );
        let script = format!(
            "$i=@('unfamiliar-addon|1.0');{};@{{installed=($i|Out-String);metadata=$metadata;metadataErrors=$metadataErrors}}|ConvertTo-Json -Depth 10 -Compress",
            include_str!("../src/inventory/chocolatey-metadata.ps1")
        );
        let scan = parse(
            Manager::Chocolatey,
            false,
            &powershell(&script, &[("ChocolateyInstall", &root)]),
        )
        .unwrap();
        assert!(scan.packages[0].evidence.library_only == Some(true));
        assert!(component_reason(&scan.packages[0]).is_some());
        write(&payload.join("real-command.exe"), "");
        let scan = parse(
            Manager::Chocolatey,
            false,
            &powershell(&script, &[("ChocolateyInstall", &root)]),
        )
        .unwrap();
        assert!(component_reason(&scan.packages[0]).is_none());
    }
    #[test]
    fn versioned_insiders_install_and_profiles_with_spaces_are_discovered_without_stable_fallback()
    {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("editor");
        let roaming = dir.path().join("roaming settings");
        let cli = app.join("bin/code-insiders.cmd");
        write(
            &cli,
            "@echo off\r\nif \"%~4\"==\"--profile=Work With Spaces\" (\r\necho vendor.shared@2.0-pre.1\r\necho vendor.work@3.0\r\n) else (\r\necho vendor.shared@1.0\r\n)\r\n",
        );
        write(
            &app.join("new-build/resources/app/product.json"),
            &json!({"quality":"insider","nameShort":"Code - Insiders"}).to_string(),
        );
        let user = roaming.join("Code - Insiders/User");
        for location in ["work", "inherit"] {
            fs::create_dir_all(user.join("profiles").join(location)).unwrap();
        }
        write(&user.join("globalStorage/storage.json"),&json!({"userDataProfiles":[
            {"name":"Work With Spaces","location":"work","icon":"briefcase"},{"name":"Inherited","location":"inherit","useDefaultFlags":{"extensions":true}}
        ]}).to_string());
        let script = format!(
            "function Get-Command {{param($Name,$ErrorAction) [pscustomobject]@{{Source=$env:FIXTURE_CLI}} }};{}",
            include_str!("../src/inventory/insiders.ps1")
        );
        let result = powershell(
            &script,
            &[
                ("FIXTURE_CLI", &cli),
                ("APPDATA", &roaming),
                ("VSCODE_PORTABLE", &dir.path().join("absent")),
                (
                    "RELEASESCOUT_INSIDERS_USER_DATA",
                    &roaming.join("Code - Insiders"),
                ),
            ],
        );
        let scan = parse(Manager::VscodeInsiders, false, &result).unwrap();
        assert_eq!(scan.packages.len(), 4);
        assert_eq!(scan.packages[1].scope, "profile: Work With Spaces");
        assert_eq!(scan.packages[1].extension.as_ref().unwrap().profile_icon.as_deref(),Some("briefcase"));
        assert!(scan.packages[3].extension.as_ref().unwrap().profile_icon.is_none());
        assert_eq!(scan.packages[1].installed.as_deref(), Some("2.0-pre.1"));
        assert_eq!(scan.packages[3].scope, "profile: Inherited");
        assert!(!roaming.join("Code").exists());
    }
}
