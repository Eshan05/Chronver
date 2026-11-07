# Portable Inventory Classification

The default sidebar excludes evidence-backed system/support records. It does
not aim for a particular count and does not use the reference catalog as an
allowlist. `Show system & support` restores hidden records and their reasons;
nothing is removed from inventory. Missing or ambiguous evidence fails open.
An installation can remain visible without a configured release-note profile.

## Evidence, Not Installed Names

- Appx: Windows inbox registration, non-removable/system flags, preinstalled
  configuration tasks, publisher, development mode and execution-alias targets.
  Ordinary non-development Microsoft packaged GUI apps are excluded by policy.
  Store registrations do not, by themselves, establish Store origin or update
  availability. Third-party Store apps are not excluded just for being packaged.
- CLI exceptions are based on the executable's PE subsystem, not an alias's
  spelling or a product-name exception list. Header reads are bounded and do not
  launch binaries. An unreadable alias target remains unknown, not definitely GUI.
- Scoop: installed manifests, declared binaries/shortcuts/PATH entries,
  dependencies, certificate data URLs and completion-data environment settings.
  A dependency with its own command or app shortcut is still a tool. Java
  runtime-only payloads are distinguished from SDKs by java/javac capabilities,
  not a list of installed distributions or version numbers.
- npm/pnpm/Bun: installed package manifests with `bin` or legacy `directories.bin`
  remain CLI tools; manifests without either are libraries. pnpm's per-package
  physical paths are used when reported, including isolated global layouts.
  Package identity/version mismatches remain unknown rather than guessed.
- Chocolatey: receipt tags identify workloads/redistributables; wrapper links are
  followed conservatively. Registered installation footprints can identify DLL-only
  support payloads after excluding the exact recorded uninstaller. Inspection
  stops at 500 entries, refuses reparse-point traversal and executes no scripts.
- WinGet: exact native display names and uniquely matched Windows uninstall
  registrations supply SystemComponent, parent-package and servicing flags.
  Native numeric versions may be zero-padded through .NET's Version parser;
  unsupported or ambiguous versions are not coerced. A registered GUI autostart
  executable without a Start-menu/desktop shortcut is classified as a background
  helper only when the executable identity and GUI subsystem are established.
- Local MSI registrations: verified cached ProductCode plus Font/File/Shortcut
  tables distinguish font-only payloads from applications. Read-only queries are
  bounded; missing metadata and mixed payloads fail open. See the
  [extension and font corrections](extension-notes.md).

The existing framework/runtime family rules remain portable fallbacks; no new
installed-product denylist was added for this policy. Description wording, fuzzy
display-name matches and equal versions alone do not classify products.

## Product Versus Installation

An OS-managed app policy follows its exact, verified product group so a WinGet
observation cannot reintroduce the same excluded Appx app. Local support evidence
does not hide a separate standalone installation of that product. Wrapper
classification can inherit its verified target's role. Raw versions, scopes,
roots, timestamps and individual candidates remain available beneath groups.

Old snapshots have unknown values for newly introduced metadata. Installed-only
startup scans refresh them; failed providers retain their previous successful
records. Optional classification failures produce warnings and preserve uncertain
entries. Some unlabelled legacy helpers may therefore remain visible: inventing
roles or suppressing unknown software to reach a target count is not acceptable.

## Insiders Profiles

Only the `code-insiders` command and Insiders data roots are used, never stable
VS Code. Discovery reads registered profile names from `User/globalStorage/
storage.json` and invokes read-only `--list-extensions --show-versions` for each
existing profile. Default, named and inherited profiles are represented. Loose
profile folders and obsolete extension directories are not treated as installed
extensions. A profile-list failure fails the provider rather than replacing its
snapshot with an apparently complete partial result.

Standard and portable installations are supported, including versioned executable
layouts and PATH shims. Set `RELEASESCOUT_INSIDERS_USER_DATA` for a custom user-data
root. A never-initialized user-data root is not created by discovery. Profiles are
bounded to 100 registrations and the registry JSON to 8 MiB; the ordinary provider
deadline/output budget also applies. Remote extension hosts are not scanned.

The same publisher.extension ID shares one sidebar group across profiles
(case-insensitively), with each profile/root/version kept as an observation.
Detected inventory has separate Software and Extensions sidebar views, each
with its own count, search and selection. Extensions never contribute to the
Software list or count; switching views preserves per-profile observations.
Marketplace candidates and
compatibility/prerelease-aware extension updates remain outside this change.

The native Insiders bootstrap is invoked without a command shell, with profile
names passed as `--profile=value` data so even option-like names cannot become
commands or install flags. Opaque command shims that conceal the native bootstrap
fail closed for shell metacharacters. They never fall back to stable VS Code.

## Verification

Local MSI receipts now inspect identity-checked cached File/Shortcut tables for
library-only payloads as well as fonts. No repair or product configuration is run.
Known framework/redistributable role labels are publisher-scoped to Microsoft's
standard product families; SDKs, ordinary apps and other publishers do not match.
An exact WinGet-to-local physical receipt link propagates support classification
to that receipt, not to every installation sharing a product name. Verified
display-group peers can contribute missing source hints without fuzzy matching,
copying installed versions, or borrowing local changelog roots.

Tests use unfamiliar product names, relocated roots with spaces, legacy/modern
Scoop manifests, ARM-selected manifest fields, isolated pnpm paths, arbitrary PE
console/GUI fixtures, inherited Insiders profiles and missing metadata. Headless
Rune scenes verify hidden/support toggling and per-profile observation switching
at minimum and desktop window sizes. A system-only machine must show an empty
detected view, not silently fall back to the reference catalog.

Primary format references: [Scoop receipt rename](https://github.com/ScoopInstaller/Scoop/releases),
[VS Code profile CLI](https://code.visualstudio.com/docs/configure/command-line),
[profile persistence](https://github.com/microsoft/vscode/blob/main/src/vs/platform/userDataProfile/node/userDataProfile.ts),
[profile inheritance](https://github.com/microsoft/vscode/blob/main/src/vs/platform/userDataProfile/common/userDataProfile.ts),
[PE format and subsystem](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format).
