# Cache Lifecycle

Chronver keeps downloaded responses, parsed notes and inventory snapshots
separate. Cached release notes never establish a manager update candidate.

Storage paths retain the former ReleaseScout name for compatibility; the rebrand
does not invalidate saved data. See [branding](branding.md).

## Inventory

Native startup loads the last saved inventory for the first frame. Automatic
scans are scheduled once the window is ready and are limited to one attempt
per provider per rolling 24 hours.
Automatic scans queue WinGet, Scoop, Store and Local first without increasing
the number of workers. Results that are ready together are applied together;
the app does not wait for slower providers to show completed results.
Recent installed snapshots count even if the candidate check was incomplete.
Attempts are recorded separately from results, so an unavailable provider does
not retry on every launch or erase its last successful inventory. Invalid or
future-dated timestamps do not block a scan. Reuse does not change the original
scan or candidate timestamps.

The toolbar scan and Inventory actions bypass the cooldown. They never install
or upgrade packages. The toolbar shows when inventory last scanned successfully.
Scoop still checks its bucket metadata freshness before querying local status.

## Parsed Notes

The Rune app saves successful parsed results under
`%LOCALAPPDATA%/ReleaseScout/notes`, asynchronously and atomically. Each observation
keeps its latest History and Range results separately, each capped at 8 MiB.
Previous single-file results remain readable. A failed refresh cannot replace
either result.

On selection, a matching saved result restores its actual query controls, report,
provenance and pagination without parsing the downloaded source again. Older
results still display as saved notes, with freshness marked unverified. Once the
window is ready, Chronver prefetches installed-to-candidate notes for confirmed
updates with a matching version-based source. The selected update goes first;
at most three jobs run at once, with at most two per primary host. Fresh saved
ranges bypass network fetching. Transient transport failures get up to two
jittered backoff retries; HTTP rate-limit cooldowns still apply. Manual queries
and Offline mode take precedence. The activity indicator shows when update notes
are loading. Activity details count the update ranges ready from a fetch or
saved result, and any attempts that returned no notes during this session.
In the default cache mode, Load notes for detected software reuses a fresh
parsed result for the exact selection, if available. Refresh bypasses it to
revalidate the source. An uncached selection otherwise has no notes until its
first successful fetch creates the parsed snapshot.

Identity includes the observation, installed/candidate versions, source profile,
installed metadata and hashed credential scope. The captured request selection,
not later draft controls, is saved. Changed profiles/installations invalidate the
result; bundled extension changelogs also require the saved file stamp to match.
Late reads cannot replace a different package, edited query or active request.

Saved results are marked cached, not newly fetched. Freshness is at most 15 minutes
and cannot exceed the original response evidence's freshness. Reopening or reading
the cache does not rewrite its timestamp. Corrupt, oversized and incompatible
entries are rejected; cache IO problems appear in Activity notices. Increment
`note_cache::REVISION` when extraction semantics change without a profile change.

## Downloaded Responses

Raw HTTP responses are stored as gzip and remain fresh for 15 minutes before
conditional validation with ETag or Last-Modified. Offline can reuse older cached
responses. Network failures and upstream cooldowns can fall back to clearly
qualified stale responses. Legacy JSON entries remain readable and are replaced
with gzip when refreshed. Successful cache writes keep up to 32 MiB of verified
raw responses per directory, removing the oldest fetched responses first.

```powershell
chronver cache prune                             # Preview cutting the CLI cache to 32 MiB
chronver cache prune --apply                     # Apply that cleanup
chronver cache prune --max-mib 0 --apply          # Remove all verified raw responses
chronver cache prune --cache-dir "$env:LOCALAPPDATA/ReleaseScout/cache"  # Preview desktop cache
```

The default CLI directory is `.releasescout-cache`. Cleanup requires no catalog or
network access. It only considers verified raw response files directly inside the
chosen cache directory, including legacy entries. It leaves saved notes, inventory,
settings, icons, rate-limit markers, and unrecognized files alone. A pruned response
must be downloaded again if needed and is no longer available to offline queries.

Regression coverage lives in the engine's inventory/HTTP tests and the Rune
app's `note_cache` and startup-discovery tests. The explicit
`saved_gcloud_parsed_cache_roundtrip` test verifies the real saved observation and
source without running inventory providers or changing the inventory snapshot.
