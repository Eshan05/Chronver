fn brand(name: &str) -> Option<&'static str> {
    Some(match name {
        "scriptc" => "ScriptC",
        "android-clt" | "android clt" => "Android CLT",
        "openssl" => "OpenSSL",
        "scoop-search" | "scoop search" => "Scoop Search",
        "qbittorrent" => "qBittorrent",
        "winget" => "WinGet",
        "nodejs" | "node.js" => "Node.js",
        "vscode" => "VS Code",
        "vscode-insiders" => "VS Code Insiders",
        "@dbml/cli" => "DBML CLI",
        "@github/copilot" => "GitHub Copilot CLI",
        "@kilocode/cli" => "Kilo Code CLI",
        "@vue/cli" => "Vue CLI",
        "@vue/devtools" => "Vue Devtools",
        "7zip" | "7-zip" => "7-Zip",
        "ffmpeg" => "FFmpeg",
        "yt-dlp" => "yt-dlp",
        "pnpm" => "pnpm",
        "npm" => "npm",
        "uv" => "uv",
        "curl" => "curl",
        "jq" => "jq",
        "yq" => "yq",
        "fzf" => "fzf",
        "ripgrep" => "ripgrep",
        "rclone" => "rclone",
        _ => return None,
    })
}

pub fn software(name: &str) -> String {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if let Some(brand) = brand(&lower) {
        return brand.into();
    }
    // Keep author-provided casing, filenames and namespaced identities intact.
    if name.chars().any(char::is_uppercase) || name.contains(['@', '/', '\\', '.']) {
        return name.into();
    }
    name.split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            if let Some(brand) = brand(part) {
                return brand.to_owned();
            }
            match part {
                "sdk" | "cli" | "api" | "ide" | "sql" | "pdf" | "json" | "xml" | "ssh" | "ui"
                | "lts" => part.to_ascii_uppercase(),
                _ => {
                    let mut chars = part.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().chain(chars).collect())
                        .unwrap_or_default()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn scene(s: &mut crate::model::State) {
    let apps: Vec<_> = ["scriptc", "openssl", "scoop-search", "qbittorrent"]
        .into_iter()
        .map(|name| serde_json::json!({"Name":name,"Version":"1.0.0","Source":"main"}))
        .collect();
    s.accept_scan(
        releasescout_engine::inventory::parse(
            releasescout_engine::inventory::Manager::Scoop,
            false,
            &serde_json::json!({"installed":{"apps":apps}}).to_string(),
        )
        .unwrap(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branding_overrides_are_explicit_and_friendly_names_are_not_recased() {
        for (raw, expected) in [
            ("scriptc", "ScriptC"),
            ("android-clt", "Android CLT"),
            ("Android Clt", "Android CLT"),
            ("openssl", "OpenSSL"),
            ("scoop-search", "Scoop Search"),
            ("qBitTorrent", "qBittorrent"),
            ("custom-sdk-cli", "Custom SDK CLI"),
            ("openssl-light", "OpenSSL Light"),
            ("FooBar UI", "FooBar UI"),
            ("@vendor/tool-cli", "@vendor/tool-cli"),
            ("vendor.extension", "vendor.extension"),
            ("example.exe", "example.exe"),
            ("pnpm", "pnpm"),
            ("uv", "uv"),
            ("yt-dlp", "yt-dlp"),
        ] {
            assert_eq!(software(raw), expected);
        }
    }

    #[test]
    fn friendly_names_leave_inventory_and_source_identities_unchanged() {
        let mut s = crate::model::State::new();
        scene(&mut s);
        for (raw, display) in [
            ("scriptc", "ScriptC"),
            ("openssl", "OpenSSL"),
            ("scoop-search", "Scoop Search"),
            ("qbittorrent", "qBittorrent"),
        ] {
            let p = s
                .catalog
                .packages
                .iter()
                .find(|p| p.detected && p.observation.as_ref().is_some_and(|o| o.id == raw))
                .unwrap();
            assert_eq!(p.name, display);
            assert_eq!(p.source_hints.as_ref().unwrap().id, raw);
            assert!(
                s.inventory
                    .scans
                    .iter()
                    .any(|scan| scan.packages.iter().any(|p| p.id == raw))
            );
        }
    }
}
