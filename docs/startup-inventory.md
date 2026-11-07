# Startup Inventory And Candidate Freshness

Native Chronver startup restores saved inventory immediately, then checks
providers asynchronously. Ordinary/headless tests do not start discovery.
The low-level `inventory::scan` API remains read-only; source refresh is the
explicit `inventory_refresh` layer used by GUI startup and Check Candidates.
No package upgrade command is executed by a sidebar action.

## Provider Policy

- Scoop: inspect configured bucket repositories through its discovered installation
  root. Skip `scoop update` when every Git-backed bucket has a FETCH_HEAD younger
  than one hour, resolves to a valid fetched commit, and matches HEAD and its
  configured upstream. An empty FETCH_HEAD left by a failed fetch is not fresh.
  This also recognizes
  recent updates made by the user or another client. Commit dates and manifest
  modification dates are not treated as refresh times.
- Otherwise run bare `scoop update` in the background, then recompute local status.
  No application name, wildcard, force, or global option is passed. Bare update
  may update Scoop itself as well as bucket metadata. Recheck freshness afterward;
  a zero exit code alone does not prove every bucket refreshed successfully.
- Dirty tracked bucket/core files, active Git locks, or unreadable metadata prevent
  automatic modification. Failures remain warnings; installed observations and
  local candidate evidence are retained without a remote freshness claim.
- Choco, WinGet, NPM, PNPM and Bun: use their existing candidate adapters rather
  than inventing a repository-refresh command. Chocolatey uses `outdated`, not
  `upgrade`; WinGet retains its normal source-refresh behavior and interactivity
  restrictions. Existing provider authentication/configuration is respected.
- Reuse a successful candidate result for 30 minutes only after locally validating
  the complete installed identity/version/source/scope/root/pin signature.
  Changed installations, expired/future timestamps, or failed checks require a
  new candidate check. Explicit Check Candidates bypasses this app-level reuse.
- Store, Local and editor profiles remain installed-only where candidate support
  is not implemented. Absence of a candidate is not proof of being current.

`Scan.candidate_checked_at` separates candidate freshness from the latest local
inventory observation. Reusing a result does not move its timestamp forward.
Old saved scans remain readable, using their original `checked_at` as the fallback.
`Scan.update_check_succeeded` describes the entire provider result. Each installed
record also carries `candidate_check_succeeded`, so a stale or malformed npm/PNPM
identity does not fail unrelated records. Failed identities keep their own diagnostic;
provider-wide warnings stay in Inventory, not every package's Info panel. Successful
per-record checks retain their receipt timestamp even when the provider result is partial.
Legacy snapshots without per-record status conservatively use the scan-level status
until refreshed. Only entirely successful, fresh provider checks are reused at startup.
There is no reliable universal signal that an external `choco outdated` or registry
query ran; do not infer it from installed-file modification dates. A recent check
is a freshness policy, not proof that the remote repository has not changed since.

## Sidebar Commands

The download icon appears beside the displayed provider badge only when that
provider has one unambiguous, supported, unpinned, confirmed newer candidate.
Its tooltip contains the exact reviewed command; clicking opens command review.
It never silently substitutes another provider from the grouped observations.
Source, scope and exact IDs come from the selected provider observation, not from
human-readable names. WinGet matching does not establish the original installer.

## Source Audit

Inspected upstream source on 2026-11-13; adapted behavior, not implementation:

- [UniGetUI Scoop refresh](https://github.com/Devolutions/UniGetUI/blob/main/src/UniGetUI.PackageEngine.Managers.Scoop/Scoop.cs): bare update, with a ten-minute in-process throttle.
- [UniGetUI Scoop command adapter](https://github.com/Devolutions/UniGetUI/blob/main/src/UniGetUI.PackageEngine.Managers.Scoop/Helpers/ScoopPkgOperationHelper.cs): bucket-qualified package ID and global scope.
- [UniGetUI Chocolatey](https://github.com/Devolutions/UniGetUI/blob/main/src/UniGetUI.PackageEngine.Managers.Chocolatey/Chocolatey.cs): candidate listing uses `outdated`.
- [UniGetUI npm adapter](https://github.com/Devolutions/UniGetUI/blob/main/src/UniGetUI.PackageEngine.Managers.Npm/Helpers/NpmPkgOperationHelper.cs): provider-specific versioned package specifications.
- [Scoop update source](https://github.com/ScoopInstaller/Scoop/blob/master/libexec/scoop-update.ps1): distinguishes bare core/bucket update from named app upgrades.
- [Chocolatey HTTP caching](https://docs.chocolatey.org/en-us/choco/features/http-caching/): provider-owned HTTP caching does not imply a separate bucket-sync command.

Tests cover freshness/identity reuse, failure/future timestamps, local-only providers,
refresh-before-status ordering, and a Windows PowerShell test with mocked Scoop/Git
commands proving recent, dirty, expired and diverged buckets do not upgrade apps.
