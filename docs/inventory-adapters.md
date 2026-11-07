# Read-Only Inventory

Inventory is separate from authored release-note extraction. It observes exact
manager IDs and raw installed/candidate versions; it never guesses a product from
a display name or merges observation versions across managers. Candidate does not imply
a safe upgrade, channel match, or availability through another installer.

## Execution

- The CLI scans one requested manager. Native Rune startup refreshes installed
  inventory from all eleven providers on background workers; headless scenes/tests
  do not scan. Candidate checks and note fetches remain explicit.
- Fixed PowerShell plans built from a manager enum, never package-name input.
- No install, upgrade, source repair, agreement acceptance, elevation or bucket refresh.
- Default 60-second deadline, cancellation, hidden subprocesses, UTF-8 output and
  an 8 MiB cap per captured pipe. Timeout/cancel stops the scan's own process tree.
- Credentials/configuration remain owned by the manager. No credentials are read
  into reports. Rune saves timestamped snapshots atomically under LOCALAPPDATA;
  these include package IDs/versions/scopes/paths and are not uploaded.
- `scan --show-plan` prints the plan without executing it. Release-note
  `--offline`/`--refresh` flags are rejected for scans; manager networking/caching
  is not controlled by the HTTP note cache.

## Providers

| Provider | Installed inventory | Candidate check | Limits |
| --- | --- | --- | --- |
| WinGet | `export --include-versions`, JSON | `list --upgrade-available` | Export omits unmatched apps. English ASCII fixed-column candidate output only; unsupported output retains installed inventory with a warning. Detection is not proof of original installer or scope. |
| Scoop | `export`, JSON | `status -l`, serialized objects | Uses local bucket manifests without fetching. Holds/global flags preserved. Identical IDs/versions in multiple scopes do not receive an ambiguous candidate. |
| Chocolatey | `list --limit-output --no-color` | `outdated --limit-output --no-color` | Probes major version; adds `--local-only` before v2. Original source/scope unknown. Limited-output lines validated. |
| npm | `root -g`, `list -g --depth=0 --json` | `outdated -g --json` | Global only. Registry/source represented as manager-configured, not inferred as npmjs. Scoped names preserved. |
| pnpm | `root -g`, `list -g --depth=0 --json` | `outdated -g --json` | Same global scope; JSON root arrays supported. Project dependencies deferred. |
| Bun | `pm ls -g` | `outdated --global` | Bun resolves its own global root/config; the reported root is retained. Empty global trees need no lockfile or candidate request. No project search. Table Latest is a candidate, not automatically the compatible Update column. |
| Store/Appx | Current-user `Get-AppxPackage` | Not supported | Excludes framework/resource registrations; includes sideloaded apps. PackageFamilyName is identity. Store origin and available versions stay unknown. |
| Local | Windows uninstall registrations | Not supported | Current user/machine registrations establish local presence, not an update source. |
| VS Code / Insiders / Cursor | Each editor's own CLI, `--list-extensions --show-versions` per registered local profile | Not supported yet | Includes inherited memberships, portable data and per-editor data overrides. Validates product identity; never substitutes stable Code for Insiders. If no CLI can be found on PATH or in the standard install locations, retained profile files alone do not count as an installed editor. A registered editor with an unavailable CLI remains a scan failure. Remote hosts are not scanned. |

Unknown versions stay unknown. A candidate is attached only to a unique exact
manager ID plus matching installed version; a race or ambiguous scope marks the
candidate check incomplete. A failed/unsupported check is not "up to date."

Scoop `status -l` omits healthy/current packages; a successful check now labels
those rows "No newer version in local buckets" rather than unknown. Reported
problems such as removed manifests stay explicit, not mislabeled current. No
bucket refresh or upstream HTTP request occurs in this check, so these omissions
are not evidence of a 429. npm/pnpm JSON travels as raw strings through Windows
PowerShell; Rust parses it to avoid PowerShell 5.1's top-level array `{value,Count}`
wrapper on reserialization.

## Rune Workflow

Detected software now drives the sidebar, independently of the original 139-row
UI/icon seed. The seed remains selectable as Reference packages and is the
fallback until detection yields records. Saved last-known rows appear while
startup refreshes providers. A successful provider replaces its previous rows,
including removals; failure retains its timestamped previous observation. Other
providers are not erased. Duplicate observations across managers are not merged.

## Apps & Tools Presentation

The default sidebar is a presentation layer over the unchanged raw snapshots:

- Appx entries are hidden only when both NonRemovable and System signature flags
  establish a system component, or their manifest explicitly exposes neither a
  visible application nor an execution alias. Manifest failures stay unknown and
  visible. A missing Start-menu shortcut alone is not grounds for hiding a package.
  Registered CLI aliases keep tool packages visible. Get-StartApps supplies friendly
  names by exact package-family identity; raw IDs remain available/searchable.
- Chocolatey `.extension` helpers require matching Chocolatey/extension receipt
  tags; KB prerequisites require a KB-number ID and Windows Update tags.
- NuGet XML is read with DTDs/external resolution prohibited, never executed.
  A single-dependency, metadata-only receipt can establish a wrapper relationship.
  Unknown directories/payloads/install scripts prevent that inference. Grouping
  follows an installed target only within matching source/scope/root/version;
  missing, ambiguous and cyclic targets do not cause records to disappear.
- `engine/data/inventory-groups.json` supplies reviewed cross-manager product aliases;
  existing exact source bindings also group their own product/component identities.
  No fuzzy display-name merging. Grouping is not proof of one installation.
- Each group retains separate selectable observations and per-observation queries.
  The default selection favors an owning manager over WinGet observation or a
  metadata wrapper. Versions and candidate states are never synthesized/combined.
- Show components restores hidden entries. The Inventory tab retains all raw
  observations. Missing/old classification metadata fails open (visible) until
  a fresh scan supplies evidence. Unknown software stays visible.
- `engine/data/component-rules.json` classifies known, source-scoped shared runtime and
  framework identities, including WinGet's XAML, VCLibs, Windows App Runtime and
  .NET desktop dependencies. These do not inherit an exemption merely because
  WinGet rediscovers them. Python, Node, PowerShell, SDKs and editor extensions
  are not broadly hidden as "runtimes" or "dependencies".

On the explicit local verification run, 381 raw observations yielded 61 hidden
components (51 system Appx records and 10 Chocolatey support/prerequisite packages)
and 310 display groups. This is not an audited count of unique applications;
unresolved cross-manager identities remain separate. No installed files were removed.

The subsequent manifest-aware run yielded 111 hidden components and 260 display
groups from the same 381 raw records. CLI-only WSL/App Installer registrations were
retained. Sidebar status reports groups/components rather than the raw total.

Exact manager/package IDs plus configured source restrictions link to the curated
product/component in `engine/data/inventory-bindings.json`. Supabase CLI, for example,
does not inherit its hosted stream. No fuzzy name matching. Binding coverage is
not universal: unmatched software still appears with notes unavailable. Matching
profiles create separate per-observation queries and installed/candidate ranges.

The Inventory tab also permits per-manager Scan installed or Check candidates.
Both update the main list and saved snapshots. Selecting a manager does not run
another check. Older results keep their originating manager/timestamp. Additional
detail rows mount 50 at a time; the main sidebar is virtualized.

Use range copies a selected record's versions into the explicitly selected
reference stream's query, without fetching or overwriting reference installation
metadata. The user must confirm the product/component match. Version records
cannot populate date-based product feeds. A missing candidate leaves the target
blank, rather than inventing a latest version.

## Verification And Provenance

Fixture tests cover schemas, scopes, unknowns, scoped npm names, Bun tables,
candidate races, unsupported output and fixed read-only plans. Windows tests parse
all 16 plan variants with PowerShell's AST parser, without evaluating those plans.
Subsequent read-only local checks verified pnpm and Insiders installed inventory,
Scoop local candidates, and an empty Bun global tree. No install/upgrade commands
were run. Other native manager/version/locale compatibility still needs testing.

Command selection was informed by UniGetUI's provider approach at commit
`e9cef69a24c30f6355d74737c9a1e648979810f7`, not a port of its C# implementation:
[npm provider](https://github.com/Devolutions/UniGetUI/blob/e9cef69a24c30f6355d74737c9a1e648979810f7/src/UniGetUI.PackageEngine.Managers.Npm/Npm.cs).
Our WinGet first pass uses its export schema rather than taking a COM dependency.
The subsequent [Windows local-inventory adaptation](windows-local-inventory.md)
adds unmatched uninstall registrations, localized Appx names and local icons;
it documents the upstream implementation inspected and remaining differences.

Primary references: [WinGet list](https://learn.microsoft.com/en-us/windows/package-manager/winget/list),
[Scoop export](https://github.com/ScoopInstaller/Scoop/blob/master/libexec/scoop-export.ps1),
[Scoop status](https://github.com/ScoopInstaller/Scoop/blob/master/libexec/scoop-status.ps1),
[Chocolatey list](https://docs.chocolatey.org/en-us/choco/commands/list/),
[pnpm list](https://pnpm.io/cli/list), [pnpm outdated](https://pnpm.io/cli/outdated),
[Bun outdated](https://bun.sh/docs/pm/cli/outdated).
