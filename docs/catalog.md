# Source Catalog

Normal setup needs no JSON. Chronver detects software on each machine, matches
known identities to its bundled profiles, and probes declared metadata when a
profile is missing. Installed versions and update candidates come from
inventory providers, not from this catalog. Discovered notes are not proof of
complete coverage, edition compatibility, or an available upgrade.

## Fixing Missing Notes

1. Get the canonical product ID with `chronver catalog` and identify the
   edition, component, and channel. Verify the actual publisher changelog.
2. If a matched product has no source profile, a URL hint is often enough:
   see the example below. For an exact parser, use [the stream example](examples/catalog-overrides.json).
   Use an existing stream ID to replace that one profile, or a new stream ID
   to add another. Other components stay intact. Keep the existing version/date
   basis and retain any known-good fallback sources you still want.
3. Validate it without fetching notes:
   `chronver --catalog-overrides catalog-overrides.json catalog`.
   Then query the intended component and inspect warnings, scope, and evidence.
   Validation covers structure, not live correctness.
4. For the Windows desktop, put the validated file at
   `%LOCALAPPDATA%/ReleaseScout/catalog-overrides.json` and reopen Chronver.
   Only use a file you trust. A malformed file leaves the bundled catalog
   intact and appears in Activity.

Overlay version 1 supports known catalog product IDs only. It does not invent
inventory bindings for new products. If a package is unmapped, propose an exact
provider ID and reviewed source in the [identity mapping](inventory-identity.md),
not a fuzzy name match. Avoid private paths or credentials in contributions.

```json
{"schema_version":1,"products":[{"id":"vsc","reference_urls":["https://example.com/insiders/notes"]}]}
```

Replace the example URL with a verified publisher URL. A hint is used by desktop
discovery when no reviewed stream applies. It does not override an existing
stream or turn an inferred source into a reviewed profile.

## Personal Tracking

The desktop optionally reads `%LOCALAPPDATA%/ReleaseScout/tracking.json`:

```json
{"schema_version":1,"products":[{"id":"gh","tracking":"excluded"}]}
```

Valid settings are `track`, `low_priority`, `deferred`, and `excluded`.
`excluded` hides a matched product from the detected sidebar without altering
the raw inventory. This file is separate from source overrides and is never
required. Desktop settings are loaded on startup; the CLI only uses explicitly
supplied catalog files. The old local-data directory name is retained for
compatibility.
