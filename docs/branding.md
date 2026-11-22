# Branding

- Primary name: **Chronver**.
- CLI command and executable basename: `chronver`.
- Reserved secondary/fallback name: **Versionaut**. This is a naming alternative,
  not a runtime mode or a second application.

The owner selected these names on 2026-11-17. Domain, package and trademark
availability have not been established.

## Compatibility

The previous project name was ReleaseScout. The shared library and desktop keep
their Rust package identifiers (`releasescout-engine`, `releasescout_engine`,
`releasescout-rune`). Historical research artifacts and test/audit environment
variables retain their identifiers.

The desktop application continues to use `%LOCALAPPDATA%/ReleaseScout` for all
saved inventory, watchlists, descriptions, icons, preferences and cached notes.
The CLI's default HTTP cache remains `.releasescout-cache`. These are deliberate
compatibility paths so rebranding does not strand existing data or trigger fresh
scans/downloads. No cache reset, preference migration or inventory rescan is part
of the rename.

The engine, CLI and Rune desktop share one Cargo workspace. The two executables
have distinct names in its shared build directory:

```powershell
cargo build --locked --workspace --bins
```

The JSON CLI is `chronver`; the Rune desktop is `chronver-desktop`.
Historical benchmark records and binaries are not renamed or rewritten.

The desktop title bar and Windows executable icon use the same custom mark
from `desktop/assets/brand/chronver-mark.svg`. The Windows MSVC build renders
the required icon sizes and embeds them with the Windows SDK resource compiler.

## Windows Launching

Run the source build on Windows with `cargo run --locked -p releasescout-rune`.
Existing per-user installations can still launch from a **Chronver** Start menu
shortcut pointing to `%LOCALAPPDATA%/Programs/Chronver/Chronver.exe`. Building
from source does not replace that executable. The workspace ships no installer.

Saved data still uses the compatibility paths above. The desktop app and engine
CLI remain separate executables; the Start menu shortcut always targets the
desktop app, not the JSON CLI.

## Preparing a Windows Release

Run the [Windows release workflow](../.github/workflows/windows-release.yml)
manually on GitHub to check the workspace and build a portable ZIP without
creating a release. To prepare a release, tag the reviewed commit as `vX.Y.Z`
using the desktop version in `desktop/Cargo.toml` and push that tag. The workflow
checks that the extracted app renders its first window on a fresh hosted
Windows runner, then creates a **draft** with the ZIP and SHA-256 checksum.

Review the notes and test the extracted app on a clean Windows machine before
publishing. The download is the desktop app only. It does not include an
installer or the separate CLI. The workflow links the MSVC runtime statically
so the portable ZIP does not require the Visual C++ Redistributable.
