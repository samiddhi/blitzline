# Python compatibility and migration

## Reference baseline

The source of truth is the working tree at
[`../../blitzline-py`](../../blitzline-py), including uncommitted and untracked
production code, documentation, presets, and tests.

At planning time on 2026-10-20, HEAD was:

```text
62498ff437183d6afe649b73ec1ba5ba3156f1de
```

That commit alone is not the baseline. The checkout contains substantial later
changes, including text-only processing and updated recovery/export behavior.

Before porting:

1. Record the reference commit, working-tree status, and SHA-256 checksums of
   relevant source, presets, example configuration, documentation, and tests.
2. Include relevant untracked files. Exclude credentials, personal recordings,
   local runs, caches, installed environments, and unrelated build outputs.
3. Run the reference tests in an isolated development environment and record
   failures, skips, dependency versions, and required external tools.
4. Capture synthetic input/expected-output fixtures for deterministic behavior,
   provider responses, review batches, exports, and identity calculations.
5. Keep fixtures and provenance in the Rust repository. Later changes to the
   Python working tree do not silently change established expectations.

Baseline capture and reference test execution have not yet been performed.

## Compatibility policy

Preserve user-visible behavior and deterministic data contracts where possible.
Do not mechanically reproduce Python implementation details or known defects.
Document any deliberate divergence with its reason and acceptance test.

Import settings; do not migrate or resume old Python jobs in v1. Users retain
`blitzline-py` for those jobs. Keep its configuration, source files, artifacts,
word lists, and history unchanged.

Preserve Anki identities for equivalent source content, language, lemma, and
note definitions. New source content or deliberately changed note schemas can
still change identity according to the existing rules.

## Initial capability inventory

This inventory establishes the port scope. Milestone A expands it into a
setting-by-setting and test-by-test coverage map.

| Capability | Rust treatment |
| --- | --- |
| Recording and UTF-8 text input | Preserve both; keep text explicitly untimed |
| Imported SRT and ASR JSON | Preserve validation, timing, and source association |
| Subtitle/plaintext generation | Preserve deterministic transformations |
| Local WhisperX | Replace with managed native transcription |
| Forced word alignment | Not a v1 requirement; preserve valid supplied alignment |
| Custom HTTP transcription | Preserve worker contract and chunk recovery |
| Direct Modal SDK invocation | Exclude; support Modal workers through HTTP |
| Hosted transcription with API key | Add a ready-to-use provider adapter |
| Blitzer extraction | Preserve subprocess JSON contract and read-only operation |
| Exact-word and word-family filtering | Preserve distinct meanings and active-input fingerprints |
| Proofreading and translation | Preserve optional stages and per-stage profiles |
| Re-listening | Preserve through selected transcription backend |
| Vocabulary review, ambiguity pass, audit | Preserve semantics and validation |
| Manual JSON review | Retain CLI access; add native direct review and guided paste |
| Browser-chat assistance | Add persisted, resumable exchanges and repair |
| TSV, APKG, AnkiConnect | Preserve all three export modes |
| Presets, custom fields/templates/styles/assets | Preserve |
| Language/deck profiles and card selection | Preserve |
| Resume, cache integrity, retry, partial results | Preserve with explicit Rust job state |
| Existing Python job directories | Leave with Python implementation |
| Python TOML configuration | Import supported settings with a report |
| Environment-variable credentials | Preserve; add desktop credential storage |
| Cleanup preview and explicit application | Preserve portable-export checks and nonresumable outcome |
| Python packaging and release helper | Replace with Rust builds and native distribution |

## Domain invariants

- Present timestamps are nonnegative integer milliseconds; text timing is
  absent, not fabricated.
- Preserve finite-time validation, permitted overlaps, source duration bounds,
  stream offsets, and segment fallback when word timing is unavailable.
- Match Python's millisecond conversion behavior at rounding boundaries.
- Source offsets count Unicode characters. Introduce explicit character/byte
  conversion in Rust and test combining characters and non-ASCII text.
- Preserve canonical whitespace behavior without silently changing source
  punctuation or words.
- Source context matching remains exact and case-sensitive. Retain repeated
  match positions and contexts crossing multiple cues.
- Keep original candidate frequencies. Review does not invent new terms,
  counts, contexts, or timestamps.
- Preserve highlight validation and escape source text before adding HTML.
- Preserve text-specific identity namespacing and source ordering.
- Corrected cues lose obsolete word alignment.

## Review and error contracts

- Require exactly one initial review decision per candidate. Kept decisions
  need a meaning and one or two valid owned contexts.
- Preserve needs-review/reject reasons and learner-facing notes.
- Keep candidates connected by shared forms together, including transitive
  groups and optional read-only dictionary-form evidence.
- Oversized groups produce actionable errors rather than silently splitting
  related candidates.
- Preserve bounded item-specific repair and accepted items across retries.
- Preserve sparse correction-only audit semantics.
- Treat transport failures separately from invalid model content.
- Unresolved review or clip errors block delivery unless partial export was
  explicitly selected; retain omissions and reasons.
- Re-listening provides transcription evidence. Text-only LLMs are never
  represented as listening to audio.

## Identity and export contracts

Port canonical JSON serialization and normalization as well as hashing.
The reference uses compact sorted-key JSON with Unicode output and SHA-256;
identity tests must verify exact reference results.

Preserve:

- Candidate IDs, note GUIDs, text/media namespaces, and Anki identity tags.
- Deck/model ID algorithms and distinctions between styling and schema changes.
- Both bundled note presets, conditional sentence cards, optional recall cards,
  typed-answer filters, ordering, tags, templates, CSS, and assets.
- TSV empty fields and escaping, accompanying definitions, media, and import
  instructions.
- APKG note/card contents and media mapping; byte-identical ZIP archives are
  not required.
- AnkiConnect model checks before mutation, skip/update behavior, scheduling,
  opt-in model updates, and per-note delivery receipts.
- Detection of changed inputs before stale export.

## Configuration import

Import supported settings, deck profiles, note customization, resource paths,
and environment-variable credential references. Resolve relative paths beside
the original TOML file and retain established explicit-override behavior.

Use separate Rust configuration/job storage. Never overwrite Python settings
as a side effect of import.

The import report must identify:

- WhisperX-specific options that do not map to the native backend.
- Direct Modal settings requiring an HTTP endpoint.
- Missing executables, resources, templates, assets, or filtering lists.
- Unsupported or ambiguous settings requiring a user choice.

Do not silently translate backend-specific compute options or embed secrets in
imported settings. Preserve legacy filtering support where it can be mapped
unambiguously; report incompatible combinations.

## CLI and lifecycle compatibility

Retain `run`, `resume`, `export`, `check`, `capabilities`, `retry-stage`, and
`clean` where their semantics apply to new Rust jobs. Keep progress separate
from machine-readable output.

Preserve exit meanings:

- `0`: complete or intentional stop.
- `1`: external operation or filesystem failure.
- `2`: invalid arguments or configuration.
- `3`: review/input required.

Add assisted-interaction operations without making the desktop application
depend on CLI output.

Preserve atomic saves, per-job writer exclusion, checksum-verified reuse,
request caching, downstream invalidation, explicit retries, and portable-export
cleanup checks. Cleanup remains opt-in and makes the cleaned job nonresumable.

## Deliberate exclusions and deferred decisions

- No v1 migration of old run manifests, caches, or pending Modal call receipts.
- No requirement for identical ASR output across WhisperX and the native backend.
- No rewriting Blitzer or building its language-pack internals in this project.
- No browser automation or required Blitzline cloud service.
- GUI framework, screen structure, minimum OS versions, and installer formats
  remain for the GUI design phase.

See [the port plan](rust-port.md#verification-and-release-criteria) for test and
release gates.
