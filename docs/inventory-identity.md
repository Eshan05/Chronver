# Inventory Identity

The sidebar groups products, not package-manager rows. A group is not proof of
one physical installation. Every observation retains its manager, raw ID,
version, source, scope, root, timestamp, candidate and query state.

## Resolution Order

1. Follow a Chocolatey metadata-only wrapper to its uniquely observed target,
   only within the same source/scope/root and exact installed version. Missing,
   ambiguous, mismatched or cyclic links fail open.
2. Resolve a reviewed binding in `engine/data/inventory-bindings.json`: exact manager/
   ID/source, an explicit manager ID family, or a local registration name pattern
   scoped to its publisher or exact official homepage. Bare names cannot bind.
   This also supplies the reference icon/name and the configured note component.
3. Otherwise use reviewed exact identities in `engine/data/inventory-groups.json` for
   products without a reference profile, such as Cursor and Obsidian.
4. Otherwise group only the same manager/ID/source. Different scopes, roots and
   versions are observations under that identity, not extra sidebar products.

Display names alone and equal versions never establish identity. No fuzzy name
matching, version coercion or reference-list allowlisting is used. Unknown
products remain visible, rather than being hidden to reach a desired count.
Stable VS Code and Insiders stay separate; Podman engine and desktop stay
separate components. Python versions share a product while their observations
and version-specific candidates remain independent. Node.js and npm are not
conflated despite their combined reference entry.

`engine/data/catalog-supplement.json` adds reviewed source metadata. Tracking exclusions
are optional [local preferences](catalog.md), not public catalog defaults,
system/support rules, or uninstall commands. Excluded observations remain in
Inventory but do not enter the detected Notes sidebar. A same-named registration
from another publisher remains visible.
Patterns are anchored and compiled when loading bindings; ambiguous matches fail
open. Publisher/homepage fields are registration metadata, not signature checks.

## Update Status

Candidate precedence is separate from identity resolution and release-note parsing.
The shared inventory comparator supports numeric revision tags, numeric package
build suffixes and numeric versions followed by Git hashes; hashes themselves are
never ordered. A WinGet installed-version upper bound confirms an update only
when the candidate is provably newer. Raw versions and stream schemes are retained.
Failed or unrequested checks cannot confirm updates; a successful per-record check
can still be used when unrelated records caused a partial provider result.

Current normally requires every observation to have a successful check. A Local
registration can share a checked provider observation's coverage only when its
actual ARP receipt ID matches that observation's explicit registration ID and both
installed versions agree. This does not merge or remove observations. Equal names,
equal versions alone, distinct receipts, failed checks and unverified Appx/Store
relations do not establish coverage. Release-note sources do not by themselves
establish an installable update candidate.

The Software UI qualifies this as **Manager** status. Unknown groups retain
specific reasons: unsupported candidate checks, not checked, failed checks,
unresolved identity, missing installed versions, or unorderable versions.
Existing Available / Current / Unknown filter semantics are unchanged.

**Publisher** evidence is independent: it identifies a public release actually
retrieved in the report's stream and selection, not the globally latest release.
Cached and retained prior results are qualified. Local changelogs and bundled
samples do not establish a public release. Displaying publisher evidence neither
enables an upgrade command nor changes the manager-update filter.

## Reconciliation Evidence

The 2026-11-11 expansion covers exact Choco/Scoop/WinGet identities for products
including Atlas, Compass, pgAdmin, Python, Ollama, Vesktop and Oh My Posh, plus
Appx/WinGet identities including Claude, Files, Notepads and Terminal Preview.
Source-specific bindings preserve Scoop bucket and WinGet source boundaries.
The five opaque msstore IDs were identified with the read-only command
`winget list --source msstore --disable-interactivity`, not guessed from their
IDs. The misleading `OpenAI.Codex` Appx identity was inspected through its local
manifest: its display name is ChatGPT and its main executable is ChatGPT.exe.
It therefore maps to ChatGPT Desktop, not the Codex CLI or a guessed Codex app.

Bindings identify products; a configured source profile is still required to
fetch notes. They do not assert that a receipt is current or that two managers
refer to the same physical installation. No install, upgrade, source refresh or
package-script execution is needed for this grouping.

## Offline Audit

```powershell
rtk cargo run --offline --example inventory_audit -- PATH_TO_SNAPSHOT
```

This reads an existing snapshot without scanning or writing it. `--details`
includes per-group IDs and versions but omits installation roots. Summary counts
separate raw observations, hidden support records, grouped observations,
matched reference products/components and additional groups. Additional groups
may include genuine extra software and identities still requiring reconciliation;
the count is not advertised as a verified unique-app total. Unobserved references
mean not seen by the current providers, not proven uninstalled.

Component classification is independent, reversible and evidence-based. The
default view hides support observations without deleting them. Tests cover
cross-manager pairs, differing versions, Store registrations, source isolation,
stable/Insiders and CLI/desktop boundaries, and preservation of every member.
See [portable classification and Insiders profiles](inventory-classification.md)
for the default system/support policy, provider evidence, and limitations.
