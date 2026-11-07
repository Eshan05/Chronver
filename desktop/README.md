# Chronver: Rune desktop

Chronver is the primary app name; Versionaut is the reserved secondary/fallback
name. Cargo builds `chronver-desktop.exe`; an existing local installation may
still be named `Chronver.exe`. The desktop package identifier and
`%LOCALAPPDATA%/ReleaseScout` storage stay unchanged so saved
inventory, preferences and caches continue to work without migration.

A bounded Windows UI trial, now connected to the shared Rust release-note engine.
Rune and Rune Kit
are pinned to `e970367bd7d2a51e2e7d58e2978ea32eac7780cb`; `Cargo.lock` pins the
remaining dependencies. No upstream modifications, Node runtime, or WebView.

Typography uses bundled Geist for the interface and Geist Mono for code. Font
files and their SIL Open Font License are in `assets/fonts/`; no system font
installation or runtime download is needed.

## Window Frame

The existing 48-point application header also serves as the window title bar;
there is no separate Windows caption strip. Rune owns the caption buttons and
non-client behavior, retaining resize edges, Aero Snap, maximize-hover layouts,
the window menu and normal close/minimize/maximize actions. The layout reserves
`Cx::chrome_insets()` rather than hardcoding button clearance, and caption icons
follow the app's light/dark theme. Narrow headers use compact search/activity
controls without removing their tooltips or actions.

The pinned Rune revision leaves a one-pixel non-client top strip on Windows 10.
DWM then draws a ghost caption above the actual window, invisible to headless
snapshots. Acrylic can also paint the otherwise invisible side/bottom resize
gutters as a grey rim. `native_frame.rs` makes the restored window fully client
painted and supplies DPI-aware resize edge/corner hits. Caption buttons and
maximized/fullscreen geometry stay with Rune.
Run `scripts/verify-native-frame.ps1` against the open app to check real DWM and
client bounds; this is read-only and does not move, resize or capture the window.

The header and sidebar try real Mica through `DWMWA_SYSTEMBACKDROP_TYPE`, then
native Acrylic on Windows 10, using Rune's DirectComposition alpha surface. A
neutral theme-aware tint is rendered over either material, so a compositor's
opposite-theme fallback cannot wash out text. The notes pane stays solid.
High contrast, disabled transparency, unavailable composition or rejected
calls keep the solid neutral fallback. Theme changes and
window focus recheck the native state; native handles are borrowed only on the UI
thread. No wallpaper imitation or screenshot processing is involved.

A separate native probe confirmed that Windows 10 rejects Mica. The Mica API
requires Windows 11 build 22621 or newer; its visual appearance still needs a
supported-host test. The Windows 10 path now tries Acrylic rather than stopping
at the solid fallback. During live moves/resizes it uses lighter native blur to
avoid Windows 10's Acrylic slowdown. See [native backdrop details](docs/native-backdrops.md).
See [Microsoft's backdrop API](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type).

## Engine Integration (2026-11-10)

Fetching is explicit: select software, choose History or Range, then Fetch notes.
History is retrieved source history, not an installed-to-latest comparison.
Range accepts version endpoints, or YYYY-MM-DD dates for product feeds. Choose a
component for compound products; channels apply to version ranges. Default uses
the engine's conditional cache; Refresh revalidates it; Offline never fetches.
Unsupported or inactive reference entries show their status without starting work.

Native startup loads the last successful local inventory when present and
refreshes installed software in the background when due. On a fresh machine,
the software list starts empty and fills as providers respond. The engine's
catalog supplies product metadata and release sources, not installed entries.
Reports show completeness, source warnings, failures and cache provenance. Copy
exports all loaded notes, including warnings. History loads 20 notes at a time:
GitHub uses one 20-release API page, with Load older requesting the next page.
Document/feed sources still download/cache a whole document before returning a
20-note window. Range results mount 50 releases at a time with Show more.
Relative Markdown links resolve
against each note's source URL, with only HTTP(S) allowed to open.

Rune's `Latest` worker pool executes the blocking engine and wakes the poll hook;
completion is applied to the requested identity, not the current selection.
Newer requests supersede older answers, and Cancel stops delivery (an already
running HTTP request may finish in the background). No fetching occurs on startup,
selection, filter changes, or headless scenes. Query/results are session-only.

The desktop uses the sibling [engine](../engine/) through the Cargo workspace.
Cache lives under `%LOCALAPPDATA%/ReleaseScout/cache` (temporary-directory fallback).
Optional `GITHUB_TOKEN` is read only by the engine worker and restricted to the
GitHub API origin. No credentials are saved. There is no updater. Startup inventory
scans are read-only; release-note fetching and update-candidate checks stay explicit.

## Explicit Inventory

Open Inventory, choose WinGet, Scoop, Chocolatey, npm, pnpm, Bun, Store or
vscode-insiders, then
Scan installed or Check candidates. Scans run in the background with cancellation
and a 60-second deadline. No packages are installed/upgraded, no sources repaired,
and no source agreements accepted automatically. Candidate checks may contact
manager-configured registries; Scoop uses local buckets without refreshing them.

Successful scans populate the sidebar automatically. Exact curated
manager/package/source bindings attach
known release-note profiles and version inputs without manual copying. Unknown
identities stay visible with notes unavailable, not guessed by display name.
The inventory table keeps all filtered observations reachable without paging,
using Rune's measured virtual rows for grouped records and expanded details.
Separate manager/source/scope/root/version observations are not silently merged.
Apps & Tools now groups verified identities and metadata wrappers, with an
observation menu preserving each provider's versions and query state. The
Show components checkbox restores evidence-backed system/Chocolatey support
entries hidden by default. Missing evidence stays visible; raw records remain
available in Inventory. Group counts are not a guarantee of unique applications.
This is automatic on native startup: background-only Appx packages are classified
from manifests, shared runtime/framework identities from source-scoped policies,
and Store names from their actual registered Start-menu names. CLI execution aliases
prevent a no-shortcut tool from being hidden. The main status shows display groups
and hidden components; raw-record counts belong to Inventory, not the sidebar.
View > Detected software returns to inventory; Rescan installed software refreshes
all eight providers. A failed provider keeps its last successful observation.
Store/Appx inventory does not establish Store origin or available versions.
WinGet export can omit unmatched apps; localized/non-ASCII candidate tables are
reported as unsupported, not as no updates. npm/pnpm/Bun scans cover globals only.
Inventory is saved atomically to `%LOCALAPPDATA%/ReleaseScout/inventory.json`,
including timestamps and local installation paths; it is not uploaded. Saved rows
are last-known observations, not proof of freshness when a rescan fails. Notes and
custom queries remain session-only. Full limitations and command provenance live
in the engine's `docs/inventory-adapters.md`.
Extensions now include VS Code, Insiders and Cursor using each editor's own CLI
and registered local profiles, including inherited memberships. Editor/Profile
filters distinguish identical profile names across applications, while one sidebar
row groups the same publisher/extension ID. Observation versions and note queries
stay independent. Remote hosts and Marketplace update candidates are not included;
Antigravity IDE is deferred rather than confused with the separate AI CLI.

All single-choice dropdowns use the reusable native `rich_select` control:
editor/profile, inventory provider, project, release component and installed
observation. The View dropdown remains a command menu. Options
can compose icons, badges and secondary text; their counts distinguish unique
extensions from profile memberships. Rune's anchored popover, modal focus and
listbox navigation primitives handle keyboard selection, scrolling and dismissal.
Scoop rows absent from its successful status check show "No newer version in local
buckets"; reported problems stay explicit. This does not refresh buckets.

## Custom Icons

The reader header opens a searchable icon picker with loaded app icons, bundled
theme-aware symbols, From file, and Automatic. It also works for entries
missing automatic icons. The monogram remains the sidebar fallback. Custom icons take precedence
without overwriting automatic icons and apply across observations of the same
product; extension profiles and package declarations use their canonical identities.
Use automatic icon clears only the saved override, never the original image.

PNG/JPEG/WebP/ICO imports reuse the engine's bounded raster decoder (1 MiB,
1024 x 1024 pixels), preserve aspect ratio, and are atomically copied beneath
`%LOCALAPPDATA%/ReleaseScout/custom-icons`. File reads, decoding and saves run on a
background worker. Choices survive rescans and restarts, even if the source image
is moved. Cancel and invalid files leave the previous icon unchanged. Personal
overrides are not read by default in tests/headless scenes; no upload is performed.

App-icon and symbol choices persist as canonical identities in the atomic local
activity file, not transient image-cache paths. App choices reference a loaded
automatic icon or direct file override without following other choices in a chain.
If that source is unavailable, the entry safely falls back to its own automatic
icon or monogram. The picker performs no network requests. Opening or canceling it
does not modify the saved preference; selection stays bound to the original entry.

## Reader Header

The Software, Extensions and Packages readers share an aligned identity header,
compact provider/version/profile metadata and one notes toolbar. Source URLs are
displayed as compact provenance labels without changing their link targets.
Long names truncate before the action group; descriptions retain the two-line
Markdown preview and delayed full preview. The note body/timeline is unchanged.

`reader_controls.rs` reuses the composable select for channel, cache policy and
tracked category. Auto cache, Refresh and Offline keep their existing fetch
semantics; choosing an option never starts a request. History/Range and
Installed/Published retain their existing query semantics. A single release
component has no redundant selector; multiple components remain selectable.
Package category editing is in the metadata row, unfollow is in the action group,
and the watchlist's view/filter/add controls share a wrapping management bar.

The provider icon factory bundles Cursor's official light/dark app icons, so
collapsible editor groups, filters and badges use the same offline-ready mark.

## Sidebar Activity

Sidebar entries size to their content with 8px vertical padding, not a fixed
height. Virtualization retains measured row heights; its initial estimate is not
a layout constraint. Page navigation uses the measured visible-row range.

Entries align a 28px icon beside a 14px title and optional one-line description.
Provider badges and profile/category context sit in a full-width line beneath
the icon and text; the installed version is
right-aligned, or authored-release age when no version is known. Release dates and
update targets remain in hover details and the reader. Neutral selection and hover
fills do not change row dimensions; the footer keeps scan context in its tooltip.

Provider-authored descriptions appear between the name and metadata, wrapping to
at most one line with the full text available on hover and to accessibility.
Blank descriptions and unresolved localization tokens leave no empty space.
Descriptions prefer saved inventory metadata, including another observation within
the same product group when needed. A reviewed, source-linked bundled catalog
fills missing Software descriptions without runtime requests. Fallback selectors
use public catalog/provider identities or exact publisher-qualified local names,
with conservative version-suffix normalization, never machine paths or installer
GUIDs. Provider text and personal overrides are retained separately and always
take priority. Extension scans also resolve descriptions
from the installed manifest and its bounded default `package.nls.json`; no extra
network request or generated description is used. Older extension snapshots pick
these up on the next successful scan. The `sidebar-descriptions` and
`sidebar-descriptions-light` scenes cover short, long and missing descriptions.

Sourced fallback text and selectors live in `assets/software-descriptions.json`.
The supporting maintainer research is private and is not included in a clone.
Only the ignored `compile_researched_descriptions` audit needs a local
`research/descriptions/` directory to regenerate the asset and coverage report.
The ignored `saved_software_description_coverage` audit uses a local inventory
snapshot without scanning, fetching or changing preferences. Normal builds,
tests and launches need neither input.

The reader's pencil action edits a local description override, limited to 240
Unicode characters with a live counter, validation, Cancel and Restore original.
Overrides share stable product/package/extension identities across provider scans,
versions and editor profiles, and save atomically with `activity.json` preferences.
Provider text is retained underneath; generated summaries are not implemented and
must remain separate from manual overrides. The `description-editor` and light
scenes exercise the dialog without writing personal settings.

Every sidebar entry has an always-visible info button that opens its details
popover without changing the reader selection. Names also open details.
Installed-version ghost buttons show the
current version plus up to three older comparable versions from loaded release
history; a provider-confirmed candidate adds a second column. These are observed
releases, not a claim of consecutive history or provider availability. History
loading reuses the existing source adapters and cache and starts only on request.
Checkboxes support session-only multi-selection and copying the selected entries;
they do not change the reader selection or install anything. Info and AI buttons
are always visible, without changing row dimensions.

Extensions have independently collapsible editor/profile groups. Each
group header uses the editor icon, a separator, then the profile's assigned icon
and name. The complete bundled Microsoft Codicons font/mapping renders profile
IDs locally (see `assets/codicons/README.md` for attribution); missing or unknown
IDs use VS Code's default `settings` glyph. Existing scans acquire assigned
profile icons on their next refresh. Profile icons do not affect membership keys.
Each installed profile membership is a separate row with its own version and query; filters still
apply, and distinct-extension counts in the editor/profile selectors are not
membership totals. Packages group by Core / Tooling (project mode uses manifest
roles, with mixed production/dev declarations treated as Core). Group counts follow
the current filters. Collapsing does not remove inventory, clear selection or move
the reader. Keyboard navigation and Select visible skip collapsed entries. Search
opens matching groups while retaining the normal collapsed state. Collapse choices
are session-only and do not add preference writes. Headers and entries share the
existing virtual list; data rows remain content-sized.

Upgrade actions only prepare copyable PowerShell commands from exact observed
provider identities and confirmed candidates. WinGet, Choco, NPM, PNPM and Bun
allow a newer specific target; availability is not checked. Scoop uses its current
manifest instead. Pinned entries and unsupported providers have no command action.
Installer origin, active registry configuration and compatibility remain unverified;
there is deliberately no execution path.

Open **AI settings** from the header or a summary action. Choose OpenAI, Anthropic
(Claude), Google (Gemini), or **Custom endpoint**, then enter a model ID and API
key. For a custom service such as DeepSeek or Qwen, enter its OpenAI-compatible
Chat Completions base URL (including its version path, such as `/v1`) and pick a
model that supports JSON mode. Local loopback endpoints may use HTTP without a key.

Choose a provider from the icon rail, then search or pick a model by name or ID.
Hover a suggestion to see its exact ID, or use the pencil to enter one directly.
The file icon opens `%LOCALAPPDATA%\ReleaseScout\ai-models.json` in
Notepad, starting with the [bundled catalog](assets/ai-models.json) if the file
is absent. In that JSON file, each provider's first ID is its default; omit a
provider to use the bundled suggestions, or set it to an empty list to use
manual IDs only. The dialog rereads the file when reopened. API keys and endpoint
URLs aren't stored in this file.

The **AI** reader tab and the sparkle menu offer a brief of the latest or up to 20
loaded releases, a summary of the installed-to-update range, and breaking changes.
The confirmation previews an approximate input token count and the number of
releases and requests included. Only clicking **Summarize** sends notes to the
displayed provider. The quick briefs use excerpts; **Summarize since installed**
and **Explain breaking changes** review all loaded authored text, grouping up to
three releases or sections per request. Markdown list items and code blocks
stay intact, and continued sections keep their heading context. Publisher-marked
breaking changes are kept verbatim; other findings are checked against the notes.
Chronver sends the planned requests in order and combines the findings. Stop the
review to prevent further requests. Missing notes and failed requests are
reported separately. Chronver doesn't send the rest of the local
inventory or switch providers on failure. Results render as Markdown in the
**AI** tab and link back to the original notes without changing the changelog.
Chronver checks that each point quotes its cited release and flags omitted points
and shortened notes. A brief of long or paged notes is not a complete review.
The connection is session-only: choose **Disconnect** or quit Chronver to clear
it. **Copy AI context** still works without a key.

**Update name** and **Update description** check known publisher metadata first
and show a suggestion with its source. Apply saves only a local display override;
it doesn't change package identity or manager records. OpenAI and Claude can make
an explicit, cited web lookup if publisher metadata isn't enough. Other providers
use the browser lookup and publisher URL option instead. Opening a summary
action loads missing notes when a source is known and keeps the confirmation open
until the user can review the token estimate.

Native scenes: `sidebar-details`, `sidebar-versions`, `sidebar-versions-light`,
`sidebar-upgrade`, `sidebar-ai-menu`, `sidebar-ai-menu-light`, and the dark/light
`sidebar-groups-extensions` and `sidebar-groups-packages` scenes.

Command references:
- https://learn.microsoft.com/windows/package-manager/winget/upgrade
- https://docs.chocolatey.org/en-us/choco/commands/upgrade/
- https://github.com/ScoopInstaller/Scoop/wiki/Commands
- https://pnpm.io/cli/add
- https://bun.sh/docs/pm/cli/add

Software display names use a small compiled branding map (`ScriptC`, `OpenSSL`,
`qBittorrent`, etc.) and conservative slug/acronym formatting, once at ingestion.
Existing friendly casing, filenames, extension IDs and scoped names are preserved;
package identities in Packages are not reformatted. Matching, source lookup and
inventory identities still use the original provider IDs.

Sidebar update indicators require a provider-reported candidate that compares newer
than the observed installed version. Published notes, project declarations and
extension Marketplace releases do not establish update compatibility. Unknown
comparisons stay unconfirmed. Hover details retain provider, source, scope,
versions, local paths, check timestamps and source errors. A failed check does not
erase its last successful timestamp; loading a cached report cannot advance that
timestamp beyond its original source fetch.

Unread counts cover retrieved authored notes, not a background subscription scan.
Opening a feed marks its loaded notes read; pagination into older history does not
create new-note alerts. Receipts are scoped to product/source/channel and captured
when the request starts. Body edits do not re-notify. The age label uses the newest
retrieved authored publication date, omitting unknown/future dates and demo samples.

`%LOCALAPPDATA%/ReleaseScout/activity.json` stores hashed read identities and last
successful check times. Saves are bounded, atomic and coalesced on a background
worker. A malformed file is left unchanged and a persistence error is shown in the
sidebar header. Tests and headless scenes do not load or save this personal state.
The `sidebar-status` and `sidebar-status-light` scenes exercise these states offline.

## Current Verification

Current verification: 48 offline UI/model tests, including real-engine offline failure
delivery, background identity safety and per-package state. The verification script
also renders loading, error, query and component-picker states at desktop/minimum
sizes. Native appearance/feel still uses owner-supplied screenshots, not Computer Use.

All 30 headless render/pixel checks pass, including automatic-filter/friendly-name
and component-restoration states, grouped desktop/minimum views,
the component toggle and open observation menu, the detected-software main view,
inventory empty/results/loading/
error states and the partial-report fixture. Manager adapters were fixture-tested;
read-only local checks also verified pnpm/Insiders inventory, Scoop local candidates,
and Bun's empty global inventory after correcting its missing `-g` flag.
The opt-in `local_startup_discovery_drives_the_main_list` test exercised the actual
startup worker/poll/snapshot path successfully against all eight local providers.
The explicit network integration check passed and remains opt-in:
`rtk proxy cargo test --locked live_github_fetch_reaches_the_rune_model -- --ignored`.
It verifies a maximum-20-note GitHub page and continuation reach the Rune model
through the same background path as Fetch notes, without running installed binaries.

The original trial notes below describe the pre-integration baseline where they
conflict with this section.

## Run

The desktop is source-only: no prebuilt executable or installer is distributed.
From the workspace root on Windows, run:

```powershell
cargo run --locked -p releasescout-rune
```

Or open the executable you built locally at `target/debug/chronver-desktop.exe`
relative to the workspace root.
Headless development scenes use engine catalog entries and labeled synthetic notes.
They do not read or simulate the owner's application inventory.

To capture a demo of the release reader without accessing local inventory, run
from the workspace root. This does not replace the live screenshot in the README:

```powershell
New-Item -ItemType Directory -Path artifacts -Force | Out-Null
.\target\debug\chronver-desktop.exe --scene readme --snap artifacts/readme-demo.png --size 1280x760 --scale 1
```

Installed software is scanned automatically in the native app; nothing is installed,
upgraded, or sent to a model. Public
release sources are downloaded only after Fetch notes; source links open only
when requested. Offline mode reads cached note responses only; it does not govern
explicit inventory candidate checks.

Filter by package or manager; sort by package or manager; select rows with the
mouse or keyboard. The split pane resizes; the navigator columns have fixed roles.
The virtualized navigator coalesces mouse-wheel notches using Rune's spring,
keeps precise/trackpad deltas direct, and respects reduced motion. Notes can be selected and
copied, or viewed as Markdown source. Ctrl+F focuses the current sidebar filter.
Ctrl+K opens the native command palette across Software, Extensions, and Packages,
with grouped local results, provider/profile context, keyboard navigation and
navigation/theme actions. It does not fetch data or run upgrade commands. Results
outside current filters are temporarily revealed with a dismissible notice;
saved filters and editor/profile choices are preserved. The
10,000-row mode repeats fixture identities as explicitly synthetic rows.
Appearance, dataset choices, and manager sorting are in the View menu. Dark is
the default. The package navigator shows identities and captured-note counts;
the reader shows recorded installation metadata without inventing version ranges.
Both appearances use shadcn's neutral OKLCH tokens from
https://ui.shadcn.com/docs/theming for surfaces, text, borders, and primary accents.
Package identity colors and the available-version status retain semantic color.
The vertical changelog timeline follows the local Resumereviews reference:
outlined 16-point markers, a connected 2-point spine, and dates above version
titles and notes. It represents chronology, not completed workflow steps.

Windows launches use the GUI subsystem, so opening the executable does not
create a diagnostic console. To retain terminal diagnostics during development:

```powershell
cargo run --locked -p releasescout-rune --features console
```

That feature changes the built executable; rebuild without it for console-free
launches. Rune's debug performance warnings are diagnostics, not crash reports.

## Verify

Close the running app before rebuilding on Windows.

```powershell
powershell -NoProfile -File desktop/scripts/verify.ps1
```

This runs formatting, tests, Clippy, a debug build, and fourteen headless renders
with nonblank-pixel checks. Images and matching element trees go in `artifacts/`.
These are functional checks, not release-mode performance benchmarks.

### Trial Result (2026-11-10)

- Initial Windows build succeeded without upstream changes, with 10 passing tests
  and seven render checks. The redesigned UI adds a custom palette, reading type
  scale, compact unstriped inventory, version hierarchy, and a View menu.
- Current composition: 288-point identity-based navigator, a compact metadata
  header, flat view tabs, and a shadcn-style connected release timeline.
  This replaces the earlier version-column layout rather than recoloring it.
- Demo and inventory tests cover absent versions, deterministic icon fallbacks,
  minimum-width headers, sorting, and collapsed-pane search.
  Timeline geometry checks confirm aligned markers, connecting lines, no trailing
  separator after the final release, and dates above their titles at both sizes.
  Neutral-theme checks guard against color casts in surfaces, text, and accents.
  The Windows executable's PE subsystem is verified as GUI (2), not console (3).
- Earlier visual review covered desktop, minimum-size, dark, PowerToys, source,
  and empty states. Current render checks additionally cover missing notes,
  uncertain installation, the long Yarn reference, app notes, and a removed app. The owner
  supplies native screenshots; do not use Computer Use for further verification.
- Windows UI Automation exposes the table, filter, controls, headings, and links.
  Ctrl+F filters the current sidebar; Ctrl+K searches the loaded library globally.
- Native screenshot capture timed out twice in the computer-use helper. Its mouse
  input then lacked capture geometry. This leaves native mouse interaction and
  window appearance unverified by automation, not evidence of a Rune rendering bug.
  A user-supplied screenshot subsequently confirmed native rendering of the initial
  UI. Repeating capture after a focus change still timed out; cause is unresolved.
- Headless tests cover row click, keyboard selection, filtering, sorting,
  clipboard output, mode changes, minimum-size controls, and bounded 10,000-row
  element output. Scroll regressions cover gradual notch motion, precise deltas,
  direction reversal, bounds, filtering during motion, keyboard paging, and row
  selection after a long scroll. Grid accessibility roles are retained.
  These tests do not establish native frame rate or perceived smoothness.
  System clipboard, text selection, scroll feel, resize feel,
  high DPI, and screen-reader usability still need hands-on Windows checks.

Verdict: continue evaluating Rune, with Tauri retained as the fallback. This is
not yet a production-framework endorsement or a hot-reload/DX benchmark.

`fixtures/demo-releases.json` contains invented release bodies, dates and links
for offline tests and headless scenes. It never represents installed versions
or published releases. Real upstream captures are not part of the checkout.

## Decision Gate

Keep Rune if real Windows use confirms comfortable selection/copy, scrolling,
keyboard navigation, DPI behavior, and accessibility, without substantial
framework work. Otherwise retain the fixtures and shared Rust engine and
replace only the UI with Tauri. The engine does not depend on Rune.

Live Rust patching, an installer, auto-update, fuzzy inventory matching,
full source coverage validation, and AI summaries remain outside this trial.

## Release Sources

The engine owns product identities, release-note streams, and source overrides.
The desktop only attaches them to detected software. Tracking preferences are
local and separate from source configuration. Release notes do not prove that
the software is installed or that an upgrade is available.

## Icons

Current behavior is an offline monogram fallback, not automatic logo discovery.
Every reference entry has an identity mark; no external icon service receives
the package list. An icon resolver is deliberately not part of this data-only step.

For a future resolver, prefer a user override, then a usable installed executable
or shortcut icon, package-supplied icon metadata, and a verified official product
asset. Cache normalized images locally with their source. WinGet's manifest
schema supports optional `Icons` / `IconUrl` data; Windows exposes file icons via
`SHGetFileInfoW`. Neither guarantees a distinct icon for every CLI or library.
Do not confuse a generic executable icon, repository-owner avatar, or company
favicon with a verified product logo. Name-only matches are particularly unsafe
for ambiguous entries.

On failure, keep the monogram and full name immediately. Later, a per-package
local-file override can replace it without changing identity or blocking notes.
Missing artwork is not an application error. No resolver or override UI is built yet.

References:
- https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shgetfileinfow
- https://github.com/microsoft/winget-cli/blob/master/schemas/JSON/manifests/v1.28.0/manifest.singleton.1.28.0.json
