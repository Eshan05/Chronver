# Windows Local Inventory

## Upstream Findings

UniGetUI was inspected at `ba1efa9febe28287b0e0842e4305c7e19a329168`.
This is a behavioral adaptation, not a C# port or a dependency on its application.

- Its [native installed-package path](https://github.com/Devolutions/UniGetUI/blob/ba1efa9febe28287b0e0842e4305c7e19a329168/src/UniGetUI.PackageEngine.Managers.WinGet/ClientHelpers/NativeWinGetHelper.cs)
  queries WinGet's composite local catalog, retains installed identifiers, and
  distinguishes catalog-backed packages from unmatched local registrations.
- Its [local sources](https://github.com/Devolutions/UniGetUI/blob/ba1efa9febe28287b0e0842e4305c7e19a329168/src/UniGetUI.PackageEngine.Managers.WinGet/WinGet.cs)
  include Local PC and Microsoft Store. Local PC is not proof of an update source
  or the original installer.
- Its [resource-name handling](https://github.com/Devolutions/UniGetUI/blob/ba1efa9febe28287b0e0842e4305c7e19a329168/src/UniGetUI.PackageEngine.Managers.WinGet/ClientHelpers/PingetCliHelper.cs)
  resolves `ms-resource:` names through Windows `SHLoadIndirectString`, instead
  of exposing those references as display names.
- Its [Windows icon handling](https://github.com/Devolutions/UniGetUI/blob/ba1efa9febe28287b0e0842e4305c7e19a329168/src/UniGetUI.PackageEngine.Managers.WinGet/ClientHelpers/WinGetIconsHelper.cs)
  uses Appx manifest logos and ARP `DisplayIcon`, as well as remote catalog icons.
  Its [icon cache](https://github.com/Devolutions/UniGetUI/blob/ba1efa9febe28287b0e0842e4305c7e19a329168/src/UniGetUI.Core.IconStore/IconCacheEngine.cs)
  separates icon acquisition from package enumeration.

## Chronver Adaptation

WinGet export remains our structured catalog-backed inventory. Unlike the full
installed catalog, export omits unmatched apps. A separate `local` provider now
reads Windows uninstall registrations, including per-user and per-machine,
native and WOW6432Node locations. It uses explicit read-only registry handles,
native display names, version, publisher, scope, installation root, and component
flags. It never invokes uninstall strings, MSI repair queries or lifecycle scripts.
It does not discover arbitrary unregistered portable executables.

Local registrations have stable `ARP\scope\architecture\key` identities.
WinGet metadata records that exact identity only after its existing unique native
display-name and version check identifies one registration. Local observations
join an existing product group only when that receipt points to one unambiguous
WinGet group. Names, equal versions, and installation paths alone are not merge
rules. Missing or ambiguous correlations remain separate, not silently discarded.
All raw observations, versions and queries survive grouping.

SystemComponent, parent-package and servicing flags feed the existing reversible
support filter. Missing flags do not prove an app is standalone or system-managed;
unknown roles remain visible. This is not a claim of complete helper/driver
classification or universal cross-manager deduplication.

Appx display names use Start-menu names, literal/localized manifest names, then
the manifest executable's native file description. The technical package identity
remains available in observation details. Optional presentation failures must not
prevent manifest role/alias classification.

Local icons use registered `DisplayIcon`, Appx manifest assets and bounded asset
qualifiers, with a native executable fallback. Paths must be local absolute drive
paths; manifest-relative paths cannot escape their package root. UNC paths,
unsupported image types, reparse-point files, oversized raster files, and nonzero
native resource indexes are not used. Rune's existing asynchronous image/icon
cache performs decoding and native icon extraction off the UI thread. Missing or
unreadable assets retain the monogram fallback. Icon paths are refreshed with
inventory. The subsequent [product-icon pipeline](icons.md) adds bounded remote
fallbacks and a separate persistent cache.

Discovery is not update support. Local records explicitly report no configured
update source rather than an invented candidate. Release-note source mappings
remain independently verified. A full WinGet COM adapter, Marketplace updates,
launcher-specific discovery are not implemented here. Remote icons are covered
separately by the product-icon pipeline above.

## Verification

`tests/local_inventory.rs` covers local provenance, unknown candidates, reversible
components, ambiguous receipt links, localized resource references and bounded
manifest asset resolution. The existing command-plan AST and read-only tests now
cover all nine providers. Rune tests cover local entries, real bitmap handles,
monogram fallback and separation from extensions. Opt-in native-artifact auditing
reconciles fresh local/Appx/WinGet observations with the saved inventory without
overwriting that snapshot or running update checks.

Windows resource contract:
[SHLoadIndirectString](https://learn.microsoft.com/en-us/windows/win32/api/shlwapi/nf-shlwapi-shloadindirectstring).
