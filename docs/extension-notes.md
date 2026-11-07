# Extension Notes

Chronver uses one extension adapter, not a handwritten profile per extension.
The Extensions sidebar groups publisher/extension IDs across VS Code, VS Code
Insiders, and Cursor. Each editor/profile/version stays a separate selectable
observation with its own note query. The sidebar shows editor badges and profile
membership; Editor and Profile menus narrow memberships before grouping. Identical
profile names in different editors are distinct. Changing editors resets the
profile filter; a profile removed by a successful rescan resets to All profiles.

One bounded local reader resolves each editor's own CLI and product metadata,
including versioned installs, nested CLI directories and supported command shims.
It enumerates only registered profiles (including inherited extensions), never
creates a profile or falls back from Insiders to stable Code. Portable data and
per-editor `RELEASESCOUT_INSIDERS_USER_DATA`, `RELEASESCOUT_VSCODE_USER_DATA`, and
`RELEASESCOUT_CURSOR_USER_DATA` overrides are supported. No remote hosts are
scanned. Profiles without detected extension memberships are not filter options.
The reader also preserves each registered profile's optional Codicon ID in scan
snapshots. IDs are bounded inert names, never paths or URLs; malformed icons do
not fail extension discovery. Missing IDs in older snapshots remain compatible.
Antigravity IDE is deferred; its separate AI CLI is not an editor inventory source.

CLI reference: [VS Code's profile-scoped extension listing](https://code.visualstudio.com/docs/configure/command-line).

## Installed And Published

- **Installed** reads the extension's bundled changelog without network access
  when available. Before reading, the installed manifest must still match the
  recorded publisher, name and version. Canonical changelog paths must stay
  inside the extension root. Missing files fall back to the exact installed
  Marketplace version, then verified repository history.
- **Published** queries the Marketplace for that exact extension identity,
  selects a publication for the chosen Stable/Preview/All channel and known
  target platform, and reads its changelog asset. Parsed sections are limited to
  versions newer than the selected profile's installed version, through that
  publication version. Native platform records take precedence over universal
  records at the same version; unknown or ambiguous matches are not guessed.
- If Marketplace notes are unavailable, a GitHub repository declared by the
  installed extension may supply a changelog. Its root or declared package
  directory must contain a matching extension manifest. Unverified monorepo releases
  are not attributed to an extension. Repository HEAD is marked
  publication/channel-unverified because it may include unpublished work.
- Link-only or sparse changelogs continue through the cascade instead of ending
  it. A bounded GitHub release fallback requires that release tag's scoped manifest
  to match both extension identity and version. Related upstream links never
  establish shared version numbering. Custom tag prefixes are discovered from
  actual releases, not extension-name exceptions. Empty release bodies stay empty.

Versioned Markdown sections use the existing document parser, preserve unknown
dates, and exclude Unreleased headings. Standalone changelogs preserve legacy H1
notices inside release sections; arbitrary documentation keeps its stricter
enclosing-heading validation. Unversioned changelogs remain readable as whole
documents, explicitly not a verified version interval. Link-only documents are
labeled release-note links, not authored changelog prose. Empty documents
are failures, not authored notes. At most 200 parsed sections are retained;
the existing reader's Show more control handles display pagination.

These are notes, **not update candidates**. Reading a changelog does not infer
that a release can be installed. Publication channels identify the artifact; a
single changelog can describe several channels. Reports retain partial/unverified
coverage rather than claiming all intervening notes exist.

## Update Checks

**Check updates** in Extensions checks the current editor and profile filters
without opening an editor. It reads local product metadata and registered
extension records, and batches exact extension IDs against each editor's own
gallery (Visual Studio Marketplace for VS Code and Insiders, Cursor's gallery
for Cursor). It compares edition, platform, pre-release preference and the
installed version. Only metadata is requested; no extension is downloaded or
installed. The check is not part of startup or the notes reader.

An update marker means Chronver found a newer gallery release compatible with
the recorded engine and platform. The editor may still apply private policies,
allowlists or cooldowns. Unrecognized product galleries, private extensions
and insufficient metadata remain unverified; a missing result is not proof of
being current.
Gallery responses use the HTTP cache, but stale or offline responses do not
confirm an update. The editor retains control of actual installation.

## Cache And UI

The reader has Installed/Published source controls, publication-channel controls
and the existing Default/Refresh/Offline cache modes. Local reads and remote work
run off the UI thread. Results are keyed to the originating observation; changing
profiles cannot transfer notes or version ranges to another observation. Source
file headings are not clickable HTTP links.

Marketplace queries use the existing bounded HTTP cache, with the POST payload
included in the cache key. Querying metadata and fetching changelog assets are
separate requests. Credentials are not sent to Marketplace/CDN hosts; GitHub
tokens remain scoped to the existing GitHub API origin. Marketplace changelog
asset URLs are restricted to its HTTPS hosts. No full VSIX download is needed.
The existing Marketplace fallback may have no matching publication for fork-only
extensions; their locally bundled notes still work. Inventory presence does not
establish cross-editor compatibility or an available update.
Installed documents are bounded to 1 MiB, and the changelog parser also rejects
larger documents. Network documents still pass the HTTP client's 8 MiB transport
budget before the stricter parser check. Failures remain visible without inventing
notes or suppressing an installed extension.

## Other Corrections

CLI-only executable icons are not trusted as product branding: compiled CLI
executables can contain the runtime's name and icon. Explicit product assets or
official-site favicons are preferred; desktop and extension local icons remain
preferred. This policy uses manifest entry-point/shortcut evidence, not a list of
runtime or installed-product names.

Local MSI packages can be identified as font-only support payloads even without
SystemComponent/ParentKeyName. Read-only cached MSI access first verifies ProductCode,
then requires every File table entry to be registered by the Font table and no
application Shortcut records. Missing/error/mixed-payload evidence stays unknown
or visible. No MSI actions, repair/configuration calls, or Win32_Product query is
used. The component toggle restores hidden records and their evidence.

## Sources And Verification

- [VS Code's changelog asset and gallery query implementation](https://github.com/microsoft/vscode/blob/main/src/vs/platform/extensionManagement/common/extensionGalleryService.ts).
- [Extension packaging and bundled changelogs](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#marketplace-presentation-tips).
- [MSI Font table](https://learn.microsoft.com/en-us/windows/win32/msi/font-table)
  and [read-only database access](https://learn.microsoft.com/en-us/windows/win32/api/msiquery/nf-msiquery-msiopendatabasew).

`tests/extensions.rs` covers offline installed notes, identity/version checks,
channel/platform selection, version windows, unsafe assets and repository scope.
Its opt-in public-network test verifies a real Marketplace changelog. Rune tests
and headless scenes cover the controls, timeline, unknown coverage and CLI icon
priority. The local provider's font-payload evidence remains independently
reversible and has unknown/mixed metadata regressions.
