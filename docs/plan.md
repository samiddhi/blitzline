# Blitzline implementation checklist

Prepared and implemented 2026-10-08. Checked boxes describe delivered software;
unresolved external acceptance checks remain unchecked. See `development.md`
for implemented contracts and the local verification workflow.

OpenRouter was added as a named provider alongside DeepSeek and custom compatible
endpoints. Stage-specific profiles, key environment variables, JSON/schema modes,
OpenRouter parameter routing, and app attribution are implemented.

Local verification uses real Blitzer and FFmpeg with an isolated dictionary and
generated recording, plus fake ASR/LLM/Anki protocols. Live ASR/API/Anki acceptance
requires provisioned services; actual APKG generation additionally needs the
optional genanki dependency, unavailable in this restricted environment.

## Basis and intended result

Reviewed `../README.md`, the actual reference script `../ref/tsvaudio2`
(the README calls it `ref/tsvaudio`), Anchor's CLI/config/core modules,
and Blitzer's CLI, processing, configuration, tests, and project plan.
Inspected both the installed `bltzr blitz --help` and the checkout executable
at `~/dev/blitzer-py/.venv/bin/bltzr`; the latter reports version 0.2.4.
Ran a small extraction with configuration and persistent writes disabled
to check the JSON contract.

The result: one command accepts local audio/video and produces source
subtitles, optional English subtitles, and vocabulary cards with matching
audio or video. With configured LLM stages, the process runs unattended.
Without an LLM, it still produces subtitles and editable vocabulary data;
reviewed structured data can then complete card generation.

- [x] Cover all nine README stages: transcription → our own SRT conversion
  → optional proofreading → transcript/optional English translation →
  Blitzer extraction → vocabulary review → deterministic validation →
  media clipping → Anki delivery.
- [x] Use JSON internally; reserve TSV for the final user-selected export.
  Replace the fragile LLM-generated TSV and tab-repair loop with validated
  structured responses and locally generated fields.
- [x] Provide three delivery modes: TSV plus media, a portable `.apkg`, and
  direct delivery through AnkiConnect. Default to TSV plus media.
- [x] Keep implementation local to Blitzline. Use Blitzer through its CLI;
  do not fork its lemmatization, mutate sibling repositories, or introduce
  an upstream change as a prerequisite.

## 1. Organization and code standards

- [x] Use Python 3.11+ for `tomllib`, Click for CLI presentation, and a
  setuptools console entry pointing to `blitzline.cli:cli`, following the
  sibling projects. Verify transcription dependency compatibility before
  setting the final supported Python range; allow a separate ASR executable.
- [x] Keep the visible root to `README.md`, `pyproject.toml`, `blitzline/`,
  `docs/`, `tests/`, and existing `ref/` (plus a license when selected).
  Put the example configuration in `docs/`, not another root file. Keep
  build artifacts, environments, caches, recordings, and run output ignored
  and outside the source tree by default.
- [x] Grow this small module map only as the corresponding stage is built;
  do not create empty architectural scaffolding:

  | Location | Owns | Excludes |
  | --- | --- | --- |
  | `blitzline/cli.py` | Commands, argument checks, progress, readable errors | Pipeline rules and provider calls |
  | `blitzline/config.py` | TOML, defaults, paths, effective settings | Executing pipeline stages |
  | `blitzline/core.py` | Explicit stage sequence and public run/resume/export functions | Parsing algorithms and presentation |
  | `blitzline/records.py` | Typed records, schema versions, shared invariants | I/O and orchestration |
  | `blitzline/transcript.py` | Pure ASR normalization, cues, text spans, SRT rendering | Launching ASR and reading media |
  | `blitzline/vocabulary.py` | Pure candidate mapping, review validation, card assembly | Subprocesses and HTTP |
  | `blitzline/media.py` | Pure clip planning and clearly separated ffprobe/ffmpeg adapters | Lemma decisions and Anki export |
  | `blitzline/integrations/` | `asr.py`, `blitzer.py`, `llm.py`: external boundaries | Owning domain records or pipeline policy |
  | `blitzline/exports/` | `tsv.py`, `package.py`, `anki.py`: delivery adapters | Transcription or vocabulary review |
  | `blitzline/storage.py` | Manifests, atomic artifact writes, cache validity | Deciding linguistic correctness |
  | `docs/` | This plan, concise setup guide, example config, developer guide | Generated run reports |
  | `tests/` | Tests grouped by stage, small fixtures | Real credentials or user media |
  | `ref/` | Historical reference script | Imported production code |

- [x] Prefer ordinary functions with explicit arguments and returned values.
  Keep text conversion, validation, mapping, clip planning, and rendering
  independently testable without files, network, environment, or clocks.
  Use small immutable records where useful; use classes only for meaningful
  state or external-resource ownership, not one class per operation.
- [x] Use guard clauses, early returns, and `continue` to keep the happy path
  flat. Normally keep control-flow nesting to two levels inside a function;
  extract a named operation when a third level obscures the sequence.
  Prefer a clear loop over nested comprehensions or clever functional chains.
- [x] Avoid global mutable configuration, implicit I/O during imports,
  generic workflow frameworks, catch-all `utils.py`, and oversized core files.
  Inject process runners, HTTP transports, clocks, and storage where needed.
- [x] Give **every function**, including private helpers, tests, fixtures,
  and methods, a straightforward docstring explaining its action. Document
  units, return values, mutation, errors, and assumptions when non-obvious.
- [x] Begin **every Python file**, including `__init__.py`, tests, and future
  maintenance scripts, with a substantive header: purpose; explicit module
  scope statement; included responsibilities; excluded responsibilities and
  their owner; and “Start here” navigation to its entry function and neighbors.
- [x] Add a lightweight AST check for missing module/function docstrings;
  review header usefulness and nesting manually. Configure formatting/linting
  in `pyproject.toml`. Do not satisfy checks with empty boilerplate prose.

## 2. User interface and configuration

- [x] Define a small CLI: `blitzline run MEDIA --language slv`,
  `blitzline resume RUN_DIR`, `blitzline export RUN_DIR --format tsv|apkg|anki`,
  and `blitzline check` for dependency/configuration diagnostics.
  These commands are implemented and documented.
- [x] Support `--config`, output directory, audio/video clip selection,
  optional proofread/translate stages, and starting from existing ASR JSON
  or SRT alongside the original media. Provide a stop-after-stage option
  for review and troubleshooting without running the whole pipeline.
- [x] Keep normal progress brief on stderr; print final artifact paths and
  summary counts. Use distinct documented exit statuses for invalid input,
  failed stages, and runs needing review. Never require a prompt mid-run.
- [x] Resolve settings as explicit CLI overrides > selected TOML > defaults.
  Find TOML via explicit path > `BLITZLINE_CONFIG` > platform config directory.
  Resolve relative paths against the TOML's directory, not the shell directory.
- [x] Configure ASR executable/model/device/language, Blitzer executable and
  optional config path, proofreading/translation/vocabulary provider profiles,
  request limits, clip padding/codecs, output format, deck name, and templates.
- [x] Allow each LLM stage its own provider/model, with a shared default.
  Store API-key environment-variable names in config; resolve secrets only
  in the adapter and redact them from logs and saved configuration.
- [x] Offer DeepSeek as the README's preferred text-workflow example, while
  validating its responses just like other providers. Do not rely on a
  provider's reputation to preserve cue counts or timestamps. Verify the
  actual selected model's structured-output and audio capabilities.
- [x] Make absent LLM configuration a supported mode: skip optional text
  services, save review input, and accept a validated manual review JSON.
  Do not fabricate translations or label unchecked candidates as reviewed.

## 3. Stable records and resumable runs

- [x] Define versioned records for source media, transcript words/cues,
  plaintext spans, candidate/context IDs, review decisions, cards, and clips.
  Use integer milliseconds internally and explicit character-offset units.
- [x] Give cues stable IDs within a transcript revision. Store exact text,
  time intervals, language, and optional confidence/word timing. Keep raw
  ASR output and a separate normalized transcript; never overwrite provenance.
- [x] Store canonical plaintext plus a span map from text offsets to cue IDs
  and available word times. Preserve punctuation and record any whitespace
  normalization in this deterministic construction.
- [x] Store Blitzer candidate ID, term, original count, context IDs, and exact
  highlight spans. Review decisions reference these IDs instead of retyping
  sentences, frequencies, or timestamps.
- [x] Keep one self-contained run directory with `manifest.json`, `artifacts/`,
  `media/`, `exports/`, and `logs/`. Use numbered stage files inside artifacts
  so a directory listing explains the order. Keep temporary chunks in cache
  and remove them only after their dependent outputs are committed.
- [x] Record source fingerprint, effective non-secret settings, tool versions,
  language-pack/config/known-list fingerprints, prompt/schema versions, stage
  input hashes, completion states, and output checksums. Write atomically.
- [x] Resume only validated matching stages. Changed subtitles invalidate
  extraction and all descendants; changed export settings invalidate exports
  only. Include provider/model settings in LLM cache keys. Cache successful
  requests to avoid paying again when a later stage fails.
- [x] Prevent concurrent writes to the same run, preserve completed artifacts
  on interruption, and distinguish complete, failed, skipped, and needs-review
  stages. Never infer completion merely because a file exists.

## 4. Media ingestion, transcription, and our own SRT

- [x] Probe the source before expensive work: readable local file, duration,
  usable audio stream, and video availability if video clips are requested.
  Check required tools and configured models; do not silently choose/download
  a different model or install a language pack.
- [ ] Implement a WhisperX adapter first, behind a small replaceable boundary.
  Establish its actual JSON schema with fixtures from a supported version;
  normalize segments and optional aligned words into our records. Map ISO
  language names/codes explicitly between ASR and Blitzer's pack codes.
- [x] Preserve the source time origin through audio conversion, resampling,
  and any chunking. If chunking is needed, record offsets and reconcile overlap
  before cue creation. Handle silence/no speech as a useful empty result.
- [x] Render SRT locally from normalized JSON, not ASR's SRT writer. Define
  deterministic cue splitting by punctuation, pauses, duration, and readable
  line length, using word boundaries where available and segment timing as a
  documented fallback. Keep reading-speed thresholds configurable.
- [x] Validate finite, ordered, nonnegative times, positive cue durations,
  bounds within source duration, and sequential SRT numbering. Preserve valid
  overlapping speech or report it explicitly; never silently reorder words.
- [x] Accept UTF-8 SRT imports with multiline text and optional BOM; reject
  malformed cues with line/cue diagnostics. Test millisecond rounding and
  hour boundaries without accumulating timing drift.

- [x] Support deployed Modal GPU functions and provider-neutral HTTP workers.
  Normalize audio locally, cache validated chunks, restore source-relative timing,
  and recover saved Modal call IDs after client interruption. Supply a deployable
  worker, documented contracts, and explicit local/remote selection. Remote
  transport tests remain offline; live GPU recognition requires separate acceptance.

## 5. Optional proofreading and English subtitles

- [x] Send proofreading bounded batches of cue IDs and text, with adjacent
  context. Request text replacements and uncertainty reasons keyed by ID;
  keep timestamp ownership in Blitzline. Record original and corrected text.
- [x] Implement “re-listen” only through an audio-capable provider or a second
  ASR pass on the questionable interval. A text-only provider can request
  re-listening but cannot perform it. Save unresolved items for review when
  no suitable capability is configured.
- [x] Reject missing/unknown/duplicate IDs, malformed responses, and unexpected
  structural changes. When edits invalidate word alignment, realign corrected
  text where supported or mark cue-level timing as the fallback; never retain
  stale word offsets as if accurate.
- [x] Freeze the final transcript revision before constructing the plaintext
  and extracting vocabulary. Downstream fields refer to this revision.
- [x] Translate batches of `{cue_id, text}` into English with neighboring
  context; demand one translated string per input cue. Render English SRT
  locally using the exact original cue IDs, order, and timestamps.
- [x] Validate complete coverage, nonempty translations for nonempty cues,
  and response size. Do not accept merged cues, summaries, or truncated output.
  Preserve both source and English SRT. Translation remains independently
  optional and is not required for source-media alignment.
- [x] Centralize bounded retries/backoff for transient failures, timeouts,
  batching, cancellation, and schema-error repair. Retry only failed items;
  save unresolved items after the configured limit. Treat transcript content
  as data, never as instructions to the model or application.

## 6. Blitzer extraction and timing correspondence

- [x] Invoke Blitzer with an argument list, captured UTF-8 stdout/stderr,
  timeout, and checked exit status. The baseline contract is:

  ```sh
  bltzr blitz --language slv --file transcript.txt \
    --freq --lemmatize --context --bold html --format json \
    --no-prompt --no-src --no-update-known --no-save-context
  ```

- [x] Add `--config PATH` only when a dedicated Blitzer configuration is
  selected; otherwise inherit its normal configuration lookup. Preserve
  exact-word (word form) and word-family (lexeme/lemma) lists, sort,
  sentence-pattern, and context-limit settings unless explicitly overridden.
  Support `skip_exact_words_file` and `skip_word_families_file` independently;
  keep legacy `known_file` for existing configurations. Explain the mandatory
  display overrides and why they never change either list's meaning.
- [x] Discover Blitzer's current `bltzr.toml` filename and fingerprint both
  effective skip files, including missing files. Refresh vocabulary on resume
  and reject stale exports after a list changes. Document the two lists with
  the English verb "to be" and singular/plural noun examples.
- [x] Require the four README flags. Also force JSON and disable report
  preambles, known-list writes, and context-history writes so automation is
  noninteractive and repeatable even when user defaults enable those features.
- [x] Validate the observed JSON array contract: `term`, integer `count`,
  optional `global_frequency`, and `contexts` containing `text`,
  `highlight_start`, `highlight_end`. Check highlight bounds and nonempty
  highlighted strings. Capture a compatibility fixture and fail clearly if
  an installed version changes this contract.
- [x] Account for an important observed behavior: JSON contexts remain plain
  text even with `--bold html`. Reconstruct escaped HTML and `<b>` spans locally
  when rendering cards. Do not search JSON for literal bold tags.
- [x] Locate each exact context in canonical plaintext and use the span map
  to recover cue IDs and clip timing. A context can span multiple cues. Keep
  all exact matches; for identical repetitions, choose the earliest valid
  matching occurrence deterministically and record the choice. Never use
  fuzzy matching or punctuation stripping to silently select unrelated audio.
- [x] Explain the CLI limitation: contexts have sentence-relative highlight
  offsets, no source offsets, a configurable limit of 1–20, and deduplication
  by sentence text. It cannot identify every surviving occurrence. Repeated
  identical sentences may provide only one distinct card example.
- [x] Stop affected candidates for review if exact mapping fails. Preserve
  context selection limits, and do not claim to have reviewed every occurrence
  from a handful of examples. Two card examples is the default target; fewer
  are valid when only one distinct supplied example exists.

## 7. Structured vocabulary review and card validation

- [x] Replace the sample prompt with a schema requesting exactly one decision
  per candidate ID: keep, reject, or needs-review; reason for rejection;
  context-specific English meaning; zero to two selected context IDs; and
  optional cognate/false-friend notes. Bundle competing candidates for the
  same highlighted occurrence together so ambiguity is visible.
- [x] Keep the prompt's substantive rules: exclude proper names and false
  lemma matches, retain valid unknown terms, use dictionary forms, avoid
  duplicates, translate only meanings supported by examples, and keep learner
  notes free of processing commentary. Require explicit accounting rather
  than allowing omitted candidates to disappear silently.
- [x] Permit dropping invalid contexts from an otherwise valid lemma. Do not
  accept invented candidate/context IDs or a context owned only by another
  candidate. Keep valid different senses when their supplied evidence supports
  them. Save uncertain cases for review rather than guessing completeness.
- [x] Preserve Blitzer's integer as `source_frequency` and the exported
  Frequency field, matching the README. Document that it counts surviving
  Blitzer candidates before semantic disambiguation; ambiguous entries may
  overcount. Do not recalculate a supposedly accurate frequency from limited
  examples or let the model invent a replacement count.
- [x] Build Word, Sentence1, Sentence2, Frequency, and media references locally.
  Sentences remain verbatim canonical text except escaped HTML plus inserted
  bold markup. Sentence2 stays empty if unavailable. Validate translations and
  notes as plain strings; escape all model/source content before HTML rendering.
- [x] Detect duplicate decisions, duplicate cards, missing decisions, invalid
  references, empty required fields, and invalid field types deterministically.
  Re-request only unresolved decisions with precise validation errors; never
  ask the model to repair TSV tab counts.
- [x] Save a readable review report and an editable structured file. Default
  to withholding final delivery until unresolved candidates are handled;
  an explicit allow-partial setting may export validated cards with an exact
  omitted-item report. Treat genuinely zero candidates as successful emptiness.

## 8. Audio/video clips

- [x] Plan clips from selected context IDs and their mapped time intervals,
  using aligned word times where trustworthy and covering cue boundaries
  otherwise. Apply configurable lead/trail padding and clamp to media bounds.
- [x] For sentences spanning cues, extract one continuous interval including
  natural gaps. Report excessively long clips for review rather than truncating
  the example. Never use the reference script's fixed 15-cue search ceiling.
- [ ] Support audio as default and configurable video with audio. Reject video
  mode for audio-only sources with a clear resolution. Verify practical codecs
  and playback in the target Anki clients before declaring video supported.
- [x] Generate deterministic safe filenames from source identity, selected
  timing, and encoding settings; reuse shared clips across cards. Avoid source
  basenames as the sole key, especially across different recordings.
- [x] Use ffmpeg with explicit stream selection, checked return codes, bounded
  concurrency, and temporary output followed by atomic rename. Choose accurate
  seeking/encoding instead of assuming stream copy cuts at exact boundaries.
- [x] Probe completed clips for duration and required streams; reject zero-byte
  or corrupt files. Cache only verified clips and invalidate changed settings.
  Record a failed clip against its affected cards, never as a working media tag.

## 9. Anki exports and delivery

- [x] Share resolved note definitions across TSV, APKG, and AnkiConnect.
  Default to the eleven-field Blitzer Basic format from `ref/example_note.apkg`,
  with conditional Sentence1/Sentence2 cards and optional typed Recall.
  Keep the original ten-field Vocabulary format as a legacy preset. Support
  customizable fields/mappings, multiple templates, CSS/JavaScript, tags, assets,
  and explicit per-deck source languages/card selections. Preserve stable
  identity and source/language metadata without displaying them by default.
- [x] TSV: use a real tab-delimited writer, documented quoting/newline rules,
  explicit empty cells, UTF-8, escaped HTML, and verified media references.
  Include media and short import instructions for field mapping, HTML handling,
  and copying media into Anki. Round-trip the output with a TSV reader.
- [ ] `.apkg`: select and verify a maintained package writer during implementation;
  keep it behind the export adapter and include all referenced media and stable
  model/deck IDs. Verify imported fields, card rendering, and media playback.
- [x] Direct Anki: use AnkiConnect against the configured local endpoint. Check
  connection, model/field compatibility, deck, and API capability before writing.
  Do not edit a live Anki collection database or user profile files directly.
- [x] Derive stable note identities from source identity, language, and lemma
  under a documented normalization policy, not generated translation text.
  Define skip/update behavior for existing notes; preserve scheduling and
  unrelated user fields. Explain that changed lemmas create new identities.
- [x] Make retries idempotent where supported. Track media uploads and per-note
  outcomes; recover from a partial batch or lost response by looking up stable
  identities before creating more notes. Never report full delivery after a
  partial failure. TSV instructions must document Anki's chosen duplicate key.
- [x] Export only validated cards with existing media. Keep the run artifacts
  available even if Anki is closed, so another export mode or later retry works.

## 10. Tests, documentation, and completion gates

- [x] Unit-test pure transformations: ASR normalization, SRT parsing/rendering,
  cue splitting, text/span mapping, highlight rendering, response validation,
  clip bounds, stable IDs, and export rows. Each test should express a behavior,
  not duplicate the implementation.
- [x] Include fixtures for Slovenian/Polish/Pali Unicode, apostrophes, combining
  marks, punctuation, repeated sentences, sentences crossing cues, ambiguous
  lemmas, proper names, empty transcripts, overlapping cues, absent word timing,
  one example, and tabs/newlines/HTML-sensitive characters in fields.
- [x] Test adapters with captured Blitzer JSON and fake ASR/HTTP/process results:
  bad JSON, missing IDs, changed schemas, rate limits, timeouts, unavailable
  dependencies, failed ffmpeg, unavailable Anki, and partial delivery.
- [x] Test config precedence and enforce that extraction never changes known
  lists/history. Test interrupted runs, stale caches, changed config/known lists,
  failed validation, and repeated exports without duplicate direct-delivery notes.
- [x] Build an offline end-to-end fixture using tiny generated audio/video,
  saved ASR JSON, deterministic LLM responses, and a temporary Blitzer pack.
  Exercise every stage without personal data or paid services.
- [ ] Separately run opt-in real-tool smoke tests: one short foreign-language
  recording through ASR and configured LLM stages; then test TSV, `.apkg`, and
  AnkiConnect in a disposable Anki profile. Listen to both example clips and
  inspect video playback, subtitles, highlighting, and repeat-import behavior.
- [x] Write `docs/setup.md` as a very short beginner guide: install prerequisites,
  pick language/known list, configure API key, choose audio/video and delivery,
  run one command, find results. Link deeper options rather than crowding it.
- [x] Write `docs/config.example.toml` with practical defaults and stage-specific
  model examples; keep detailed schemas, module navigation, resume rules, and
  troubleshooting in `docs/development.md`. Link setup and this plan from README.
- [ ] Acceptance: a configured run produces valid source SRT, optional English
  SRT with identical timing, reviewed cards with exact examples, playable media,
  and each requested export; a retry reuses valid work; failures identify a
  specific stage and leave enough evidence to resume.

## 11. Delivery order

- [x] **First slice:** package/config/CLI, records and manifest; existing ASR
  JSON or SRT → canonical transcript → real Blitzer JSON → editable review JSON.
  Gate: no lost timing correspondence and no changes to Blitzer user data.
- [x] **Second slice:** validated manual review → audio clips → TSV plus media.
  Gate: a complete offline path works before introducing model variability.
- [x] **Third slice:** ASR adapter and automated vocabulary review, including
  bounded repair and resumability. Gate: one command produces the same artifact
  contracts as the manual path and exposes unresolved work accurately.
- [x] **Fourth slice:** optional proofreading/re-listening and translation.
  Gate: corrected transcripts invalidate descendants correctly, and translated
  subtitles cannot change cue identity or timing.
- [ ] **Fifth slice:** video clips, `.apkg`, AnkiConnect, packaging, concise user
  guide, and all completion checks. Gate: all README options are exercised;
  the first working slice is not treated as completion of the full pipeline.
