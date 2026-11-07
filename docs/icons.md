# Product Icons

## Source Order

1. Installed Appx assets, native Windows icons, Scoop shortcut/executable targets,
   or an Insiders extension's bundled icon.
2. Explicit icon URL in package metadata or the UI catalog's `icon_sources` plan.
3. Official product website icons declared by HTML `link rel=icon` or
   `apple-touch-icon`, then `/favicon.ico`.
4. Existing monogram fallback.

For CLI-only packages, embedded executable icons are skipped: they may identify
the compilation runtime rather than the product. Explicit product metadata and
official-site branding are preferred. Desktop shortcuts and bundled extension
icons retain local-first priority.

npm/pnpm/Bun installed manifests supply homepage URLs. Chocolatey nuspecs supply
project/icon URLs; Scoop manifests supply homepage URLs. Exactly matched curated
release-note reference pages provide a website fallback when package metadata is
absent. No product website is guessed from an executable or package name.
The reference fixture schema accepts `icon_sources: {icon_url, homepage}` for
reviewed explicit overrides, without an embedded icon-name denylist.

GitHub/GitLab/Bitbucket and package registry pages are not treated as product
websites: their generic favicons and owner avatars are not product icons. An
explicit product asset hosted on a forge/CDN is allowed. No favicon proxy or
inventory-upload API is used. Fetching a site's icon necessarily contacts that
site; it is not a completely offline operation.

## Extensions

Only installed IDs/versions returned by the Insiders profile CLI are eligible.
Icons are located through the profile/default registered `extensions.json`, not
by scanning old extension directories. The physical package manifest must match
publisher, name and installed version. Relative icon paths must remain inside
the extension directory. Missing/invalid metadata cannot erase an observation,
change profile membership, or trigger an install. Custom extension locations are
supported when represented by those registrations; remote hosts are not scanned.

## Fetching And Cache

The desktop app reads disk entries in bounded batches on a dedicated disk-only
worker, independently of its two network workers. The selected product and
onscreen rows come before the remaining expanded rows in the current section/filter.
Custom icon reads use the same priority order. Headless scenes and ordinary tests
never enable network fetching. Requests/results are bound to source plans, not whatever
row is selected when a request finishes. Verified group members share local
assets before using network fallbacks. Working local icons are not displaced by
remote results.

The icon cache is separate from note/inventory caches, under the legacy
`ReleaseScout` local app-data directory. Source plans key positive and negative entries; positive
results are fresh for 30 days, failures one day. Valid expired positive entries
display immediately while their refresh runs in the background; a failed refresh
does not erase them or replace their metadata with a negative entry. Each source
plan is fetched at most once per app session. Repainting does not refetch. Cached
source URLs provide provenance, and missing/corrupt cache files are recoverable.
Inventory refreshes retain working remote image handles when their source plan
has not changed. Native executable extraction remains asynchronous and in-memory;
it is not part of this remote PNG disk cache.

Downloads require credential-free HTTPS on the standard port. DNS answers are
validated and pinned, private/local addresses rejected, redirects revalidated,
and ambient proxies/cookies/GitHub credentials are not used. Body size is capped
at 1 MiB; oversized HTML pages use only a bounded prefix for icon discovery,
while oversized image responses are rejected. Legacy negative entries are retried
once after this discovery correction; successful cached icons are retained.
At most four declared icon candidates are tried, with per-request
timeouts and elapsed-time checks between fallback requests. OS DNS resolution
still follows the system resolver's own timeout behavior.

PNG/JPEG/ICO/WebP use a standard image decoder with 1024px dimension and 16 MiB
allocation limits. Remote SVGs use resvg's static renderer, capped at 256 KiB,
1024px intrinsic dimensions and 4,096 visible nodes; filters and referenced
images are disabled. No scripts, fonts, files or secondary URLs are loaded by
the vector renderer. Both paths normalize to at most 96px PNG before caching.
Local uploads still accept raster formats only; animated icons are not supported.

## Package Libraries

Rune's personal npm watchlist and public project dependencies use the same icon
resolver/cache. Installed public manifest homepages are used immediately; a small
exact-name map supplies verified product homepages for common frameworks. Other
public npm names receive background, identity-checked registry metadata lookups,
cached for seven days. Only the declared product homepage is handed to the icon
resolver, not npm's own favicon, a GitHub owner avatar or an arbitrary README image.
Private/non-registry project dependencies never enter the public metadata queue.
Custom overrides retain priority; unavailable product icons retain monograms.

## Verification

`tests/icons.rs` covers source order, HTML-relative links, positive/negative disk
caches, network-free reads, stale preservation after failed refreshes, invalid/unsafe
URLs, and an explicit official-site integration check.
`tests/classification.rs` covers registered extension version/path confinement.
Rune's loader tests verify disk hits with both network workers deliberately blocked,
viewport priority (including grouped rows and outdated ranges), source-bound
application, local precedence, inventory-refresh handle reuse, and disabled-by-default
test networking. Existing headless icon/fallback scenes still exercise painting
without live HTTP.
