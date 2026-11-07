# Packages

The Rune trial's Packages section opens a personal Watchlist, independent of
Software, Extensions and project installations. Track npm packages by name, choose
Core or Tooling, and unfollow entries without editing a manifest. Both categories
appear as reusable badges in sidebar rows and the reader header. Cargo is deferred
in the current UI: saved Cargo entries remain intact but are hidden, and adding
packages targets npm only. The shared engine retains its Cargo reader for later.
The watchlist is saved locally in `ReleaseScout/watchlist.json`. It contains only
ecosystem, name and category, not project paths or supposed installed versions.
There is no bundled machine-specific seed: the owner's approved audit candidates
were explicitly imported into their personal store. Other machines start empty;
an explicitly empty saved watchlist stays empty.

Load notes checks the selected package's public registry identity, then reuses the
shared, cached release readers and guarded npm source discovery. Reviewed Cargo
release sources cover Serde and Serde JSON; other Cargo entries still expose
registry publication metadata, without guessing a repository-wide changelog.
When authored notes cannot be retrieved, registry versions remain explicitly
metadata-only. Nothing is installed and no project compatibility is inferred.
Only selected entries are fetched; adding a list does not trigger bulk requests.

Project scan remains an optional view. Open a project folder or enter its path and
scan. The last selected folder is saved in `project.json`; startup restores the
path but does not automatically scan it. Headless scenes and ordinary tests do
not load or save either personal preference. Explicit saved-watchlist scenes are
available for local, read-only verification.

The first adapter reads `package.json`: runtime, development, optional and peer
declarations, including nested monorepo projects. It groups a package in the list
while retaining each project's declared range and installed version separately.
The project selector narrows the list; the observation menu switches declaration
contexts. Cargo, Python and Go adapters are not implemented yet.

Scanning does not run package managers, lifecycle scripts or project code, change
manifests/lockfiles, or contact a registry. It skips generated/vendor directories,
does not traverse directory symlinks, and stays inside the selected canonical root.
Limits: 12 directory levels, 20,000 directories, 512 manifests, 10,000 declarations
and 1 MiB per manifest. Incomplete/invalid scans expose warnings, not fake versions.
Installed metadata comes from nearest `node_modules` up to the selected root;
in-root pnpm links work, while external links are unresolved. Lockfile-only versions
are not resolved yet. `^16.0.0` is a declaration, never an installed version.

Load notes is explicit. It reuses the cached, public-only HTTP client and shared
source-discovery/readers. Reviewed npm identities for Next, React, Vue and Nuxt
live in `engine/data/package-sources.json`, separately from software profiles. Other npm
packages use exact installed metadata or an identity-checked public npm registry
response, then the existing guarded repository/changelog cascade. Registry lookups
send only the selected package identity/version, not project paths or manifests.
Custom/private registries and credentials are not queried. Local/file/workspace/git
declarations do not probe public registries or acquire a public namesake's profile;
only an identity-checked local installed changelog can supply their notes.

The first matching history page is bounded and explicitly partial. Stable/Preview/
All filters are applied to actual note flags; at most three source pages are
checked if earlier pages contain only another channel. Atom fallback requires All
because GitHub publication-channel flags cannot be verified there. Release history is not a
compatible update candidate: constraint satisfaction, lockfile changes and editor/
runtime compatibility are not inferred. Refresh and Offline use the existing cache
policies. Rescanning invalidates dependency-note sessions without disturbing
software/extension inventory, and background inventory refresh retains dependencies.

Extension reader regressions also cover linked version headings, legacy top-level
notices inside a release and pointer-only changelogs. A pointer can fall through to
the extension repository's GitHub release only after a scoped manifest at that tag
confirms both publisher/name and version. Related upstream links are not treated as
the extension's version history. A release with an empty body is reported honestly
as having no authored notes.
