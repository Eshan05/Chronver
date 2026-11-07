# Engine Review Disposition

2026-11-10: the owner-requested single GPT-6.1 Terra High review completed.
It was read-only; the parent implemented and verified fixes. No independent
second review was requested or performed.

| Finding | Resolution | Regression |
| --- | --- | --- |
| P1: Same-level headings truncate a release | Preserve non-release subsections; reject ambiguous enclosing headings explicitly | `same_level_subsections_are_preserved_and_parent_ambiguity_is_rejected` |
| P1: Empty fallback displaces partial notes | Rank nonempty history ahead of empty history, then completeness/count | `empty_complete_fallback_does_not_erase_partial_posts` |
| P1: Channel-mismatched endpoints look complete | Check endpoint channel membership; cross-channel queries return partial | `mismatched_channel_boundaries_are_partial` (SemVer suffix and GitHub metadata) |
| P2: Feed edits treated as publications | Use published only; unknown-date items retained in inspection, excluded from dated windows, with partial coverage | `feed_edits_are_not_new_publications` |
| P2: Later metadata errors discard fetched history | Retain notes/evidence with invalid dates; stop malformed/unsafe pagination and warn without following it | `malformed_later_metadata_keeps_earlier_notes_and_evidence` |
| P2: Package qualifications omitted; T3 defaults to stable | Include package/stream caveats even on failure; T3 profile defaults to preview and matches only versioned nightly tags | `scope_caveats_survive_success_and_source_failure`, bundled catalog test, explicit nightly CLI range |

Verification after fixes: 31 offline tests, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, and `cargo build` pass.
The default 24-source smoke rerun passed: all returned authored ranges, with
19 complete and five explicitly partial. Its generated
reports are under ignored `artifacts/live/`. A separate T3 Code nightly range
uses 0.1.9-nightly.20261110.4449 -> 0.1.9-nightly.20261110.4452, not a machine
inventory assertion. The release API contains stable and maintainer-preview tags,
which this nightly profile intentionally excludes.

Residual scope is unchanged: 77 entries configured, not all live-validated; some
active sites still need profiles. Completeness is conservative and conditional on
curated profile correctness and upstream history. UI integration, local inventory,
upgrades, icon resolution and model summaries remain unimplemented.
