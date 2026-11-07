# Software Source Discovery

Release-note parsers are not identity discovery. The native UI now starts a
bounded lookup when selected software has no reviewed extraction profile.
Extensions continue to use their separate installed/Marketplace adapter.

## Cascade

1. Existing reviewed manager/product/stream bindings win. A binding may be
   shared across an unambiguous, evidence-backed observation group, never by
   fuzzy display-name matching.
2. Reviewed reference URLs and explicit package release-note URLs are probed
   as versioned or dated documents. GitHub blob references use raw content.
3. Installed Scoop manifests, Chocolatey nuspecs and npm-family package.json
   metadata supply repository, download, website and component-directory hints.
   Exact community WinGet IDs can obtain ReleaseNotesUrl/PackageUrl from their
   exact installed-version locale manifest in microsoft/winget-pkgs. Missing
   en-US locales fall back through the version manifest's declared DefaultLocale.
   Identity-checked installer manifests can supply declared GitHub download
   identities when locale metadata has no release-note/repository URL; installer
   binaries are never downloaded. Four-part Windows versions ending in zero
   can match their equivalent three-part release tag.
   That packaging repository is not treated as the product's repository.
4. Declared GitHub repositories are checked for authored releases, then known
   changelog files. Download tag templates preserve component namespaces;
   repository-name-prefixed tags are recognized without guessing other components.
   An unavailable GitHub API can fall back to that repository's official Atom
   feed. Atom snapshots never establish full history or GitHub channel flags;
   channel-specific version ranges require verified metadata or an All-channel query.
   npm-family repositories require package.json identity at the declared
   directory; whole-monorepo notes are refused for mismatched components.
5. Validated installed npm-family changelogs are an offline snapshot fallback.
   Name/version must still match package.json and canonical files must remain
   inside the package root. Sections newer than the installed version are omitted.
6. The product website can explicitly link same-origin changelog/release-note
documents or RSS/Atom feeds. A website page identified as release notes by its
path/title is also probed; a version heading on a marketing page is insufficient. Arbitrary
   guessed /changelog URLs, repository-name guesses and cross-origin crawling
   are not used.

A source is promoted only after a probe produces authored notes. Declared
same-basis alternatives are retained for later reader failures or sparse bodies.
A successful
profile enables ordinary paged history fetching; it does not eagerly fetch all
notes. Date streams never receive installed version strings as date boundaries.
Automatically discovered profiles always report partial/unreviewed applicability,
not confirmed update compatibility or complete component coverage.

## Bounds And Cache

Only the selected unmapped software starts a lookup, with one worker. A lookup
permits at most 12 source reads and stops initiating reads after 30 seconds;
an in-flight read may take another 12 seconds (DNS also uses OS timeouts).
Discovery successes are cached for a day and failures for an hour, keyed by all
identity/version/source hints. HTTP bodies retain the shared validator cache.
Retry bypasses the discovery and HTTP freshness caches. Offline callers may
reuse matching stale discovery entries without network access.

Automatic HTTP requests and later source reads use credential-free public HTTPS
URLs, public DNS validation with pinned addresses, no environment proxies, and
bounded HTTPS redirects. Every redirect target is validated and pinned; tokens
are never forwarded across origins or introduced by a non-API redirect to GitHub.
Canonical redirect URLs are retained for correct relative-link resolution.
GitHub tokens remain API-origin scoped.
Local file sources are a separate, identity-checked reader, not HTTP file access.

Rate-limit responses honor Retry-After (seconds or HTTP-date) and GitHub reset
headers via credential-scoped persistent cooldowns. Cached bodies survive
network errors, access/rate limits and server failures with explicit stale-cache
evidence and partial reports. Refresh does not override an upstream cooldown.
Old discovery cache entries are invalidated when the discovery schema changes.

The opt-in html_entries document format uses a reviewed CSS entry selector,
bounded to 200 entries, rather than conflating every product on a combined
changelog. Codex CLI and ChatGPT desktop use different entry selectors. HTML
disclosures are normalized to readable content rather than leaking widget markup.

Lookup results only attach to current matching metadata, survive provider refresh,
and cannot replace reviewed profiles. Headless scenes/tests do not enable
network discovery. No metadata means an explicit missing-hints state; unsuccessful
probes show a retryable no-usable-source state, not an unconfigured-profile claim.

## Limits

Public notes cannot be invented when a publisher supplies no usable source.
JavaScript-only changelogs, unusual heading layouts, private sources, missing
installed-version WinGet manifests and undeclared cross-origin changelog sites may
still need a reviewed profile. npm package names are not sent to a guessed public
registry, and Store/local app names are not used as web-search identities.

Tests: `tests/source_discovery.rs`, UI `src/source_loader.rs` and native runtime
render regressions. `RELEASESCOUT_LIVE_SOURCE=1` enables the opt-in public-source
smoke test; ordinary tests remain offline.

`rtk cargo test --test live_fallbacks -- --ignored --nocapture` forces GitHub API
failure while verifying real Bun, Neovim, Codex CLI and desktop fallback retrieval.

## Upstream Distinction

UniGetUI's native WinGet path checks IsUpdateAvailable and reads ReleaseNotes,
ReleaseNotesUrl and PackageUrl from catalog metadata; this differs from querying
GitHub for every installed app. [Source](https://github.com/Devolutions/UniGetUI/blob/ba1efa9febe28287b0e0842e4305c7e19a329168/src/UniGetUI.PackageEngine.Managers.WinGet/ClientHelpers/NativeWinGetHelper.cs)

Releasebot describes manually verified sources, source-specific transformations,
scheduled updates and review of broken feeds. It explicitly excludes sources
its systems cannot access. It does not promise a universal blocker-free scraper.
[Methodology](https://releasebot.io/methodology)

## Actual Inventory Audit

### Shared Source Formats

Reviewed source definitions reuse the same engine adapters:

- HTML/Markdown sections, with optional named `body` captures for inline release
  prose and `prerelease` captures for explicitly labelled beta releases.
- Preformatted HTML text (`plain` plus a CSS selector), preserving line breaks.
- `json_markdown` with an explicit JSON pointer and at most one array wildcard,
  such as `/notes/*/content`. Only selected string content is parsed; schema drift
  fails explicitly. Snapshots are bounded to 200 entries and remain partial.
- Linked version pages, including bounded release-line pages with multiple patch
  sections. Additional documentation origins require an explicit allowlist.
  Later-page failures retain earlier notes as partial evidence.
- HTML exclusions remove source-specific tooltips/metadata before extraction,
  preventing aggregation first-seen timestamps from becoming publication dates.
- Markdown/HTML commit-description tables count as authored prose, but asset/
  checksum tables do not. GitHub mixed HTML/Markdown is normalized before reading.

Automatically discovered version streams use numeric ordering to retain two-part
headings alongside installed three-part versions. Coarse headings do not establish
patch-level history. Numeric versions also support annual distribution years.
Reviewed references and declared release-note documents precede generic
repository probes. A GitHub blob URL is read as raw content. Component-scoped
installer tags must match a whole installed version; the same repository's
unscoped releases and root changelog are not used as fallback for that component.
Explicit monorepo scopes
remain necessary when package names differ; this does not relax the automatic
repository identity check or substitute editor release notes for a CLI.

Firefox's reviewed stable and Developer Edition streams follow their official
latest-page redirects, not a fixed major number. Postman's app JSON source is the
public CDN endpoint fetched by its release-notes page, not its separate CLI feed.
Canva uses the owner-selected Releasebot aggregation with explicit product-wide
scope, not Canva SDK or Windows binary version claims. TeX Live uses annual
distribution changes from its official guide, not all installed CTAN packages.

`rtk cargo test --test source_corrections -- --ignored --nocapture` explicitly
verifies supplied sources over the network. Ordinary regression tests use local
fixtures and never fetch public pages.

From the Rune workspace, run:

```powershell
rtk proxy cargo test --offline --locked -j 1 source_audit::real_software_source_coverage -- --ignored --nocapture
```

This explicitly enabled test refreshes read-only installed metadata, builds the
same default Software groups as the UI, and probes discovery/history for every
group. It leaves the native inventory snapshot unchanged. Reports and incremental
checkpoints live in `artifacts/source-coverage-audit/` in the Rune workspace.
The raw inventory artifact is machine-specific and must not enter shared memory.

`authored_notes` means the engine returned recognized authored bodies on the
queried history page, not human confirmation of every inferred product/channel
mapping. Inspect per-stream scope, warnings, attempts and evidence timestamps.
An empty first page can still have older history. `not_found`/`lookup_failed`
mean this cascade did not resolve a usable source, not that no public notes exist.
Access/rate-blocked origins stop further network requests during the audit;
matching cached documents can still be read, with cache provenance retained.
