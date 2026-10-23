# Native implementation status — 2026-10-21

This records implemented behavior and remaining acceptance work. The approved
architecture and GUI design remain the target; passing synthetic tests is not a
claim that every release gate is complete.

For subsequent import, persistence, profile, help, and demo-boundary corrections,
see [production-ui-audit.md](production-ui-audit.md).

## Reference baseline

`tests/fixtures/reference/provenance.json` records the Python working-tree HEAD,
status, and source/document/preset/test hashes. The reference environment's full
suite passed **184 tests**. The system interpreter run passed 170 with 14 skips;
the reference virtual environment supplied the export dependency and removed
those skips. Reference files were not modified.

Synthetic fixtures cover Unicode text/candidates, canonical hashes, identities,
review decisions, note fields/model IDs/deck IDs, and genanki-produced APKG
schemas, notes, templates, and conditional card ordering. The default Blitzer
note preset is embedded from the captured reference. No personal data is bundled.

## Implemented capabilities

| Area | Implementation and evidence |
| --- | --- |
| Native runtime | Rust executables; Reqwest/Rust TLS; bundled SQLite; native ZIP writer; no Python import, interpreter, pip, or conversion fallback |
| CLI | run, resume, export, check, capabilities, prompt, submit, show, list, pause, retry-stage, clean, resources; JSON output and actionable exit codes |
| Input | Explicit UTF-8/BOM text, timed media, imported SRT/ASR JSON, Unicode character offsets, cue validation, partial-alignment fallback, stream-offset/chunk rebasing |
| State | Per-run exclusive lock, versioned atomic manifest, stage/output hashes, source checks, dependency fingerprints, saved partial exchanges, downstream invalidation, recovery |
| Extraction | Compiled subprocess contract with read-only flags; active filters/config/pack fingerprints; optional imported Blitzer JSON; compiled fixture verifies flags with no interpreter on PATH |
| Review | Owned-context/meaning validation, no invented terms, shared-form grouping, optional SQLite form evidence, independent ambiguity pass, sparse audit patches, manual JSON edits |
| Browser chat | Persisted IDs/revisions, complete schemas, fenced/plain JSON, transactional rejection of stale/unknown/duplicate/conflicting replies, retained valid items and repair prompts |
| AI APIs | OpenRouter/DeepSeek/compatible HTTP, per-stage profiles, response modes, routing, reasoning, caches, bounded retries and separate semantic repair; tested with local servers |
| Transcription | whisper.cpp with preconverted model, Groq multipart, base64 HTTP worker, normalized mono PCM WAV chunks, caches, second ASR for uncertain corrections |
| Clips | Native FFmpeg/FFprobe, selected audio stream, padded/capped intervals, shortening and retiming, verified clips/receipts and checksum-based rebuild |
| TSV | Real portable table, directives, note definition/templates/CSS/assets/media and import guidance |
| APKG | Rust SQLite/ZIP package writer; fixture comparisons for both presets with recall on/off; stable note/model/deck IDs and conditional card ordinals |
| AnkiConnect | v6 preflight, configured skip/update, stable tags plus legacy identity fallback, media, per-note delivery receipts, field updates without scheduling mutations; fake-service recovery tests |
| Cleanup | Preview by default, verified portable-export requirement, symlink/traversal rejection, crash-recoverable application, retained exports/original source, nonresumable receipt |
| Resource installation | Explicit checksum-pinned catalog, platform selection, HTTPS download, optional ZIP entry, native executable checks and installation receipts; no arbitrary language-pack format guessed |
| Desktop | Existing native chrome/design retained; normal projects use the native pipeline on a background thread; pause/reopen/refresh, browser exchanges, direct word review, real exports, native config/tool checks, CLI installation |
| Demonstrations | Try an example remains isolated and labeled; real projects cannot enter the simulation tick or generate canned answers |

## Important boundaries

- Compiled Blitzer is an external project. Existing local Python launchers are
  deliberately refused. Imported vocabulary can exercise the native pipeline
  now; a compiled Blitzer release must pass the same contract before bundling.
- Native recognition and cue timing can differ from WhisperX. Forced alignment
  parity and the direct Modal SDK are not v1 requirements. HTTP Modal workers
  remain supported through the worker contract.
- Browser reply submission does not initiate another paid request. Explicit
  processing/resume can invoke configured second ASR when re-listening is enabled.
- Accepted direct review edits create a new revision. Unsaved GUI word drafts
  block preview/export until saved; text corrections preserve original source
  identity. The recording editor currently requires corrected subtitles in a new
  project rather than a whole-document timing rewrite.
- CLI configuration supports custom fields/templates/styles/assets and deck
  profiles. GUI design defaults apply to new projects; card preview stays plain
  text, and the system media player handles playback. Managed word lists are
  captured when a new project selects their profile.
- Desktop-entered model and transcription keys are session-only and scoped to
  their respective connections. Other configured profiles use environment credentials.
  OS credential-manager integration remains pending.
- The desktop workspace retains its original application-data directory for
  continuity. Each native project has a separate run folder. CLI and GUI may
  inspect the same native runs, but only one writer owns a run at a time.
- Python configuration is not copied wholesale: unsupported fields fail with an
  explanation. Python jobs are not resumed or rewritten. The legacy profile
  importer remains explicitly limited; the native config picker validates the
  native configuration separately.

## Validation

Local verification on macOS completed with **76 Rust tests passing after the production UI audit**, formatting
checks passing, and Clippy passing with warnings denied. Both the macOS app
bundle and standalone CLI archive built successfully. The bundled CLI executed
with an empty PATH. Native-window captures of the assistant and settings were
inspected in light and dark modes; the native toolbar and established layout
remain intact.

Rust tests cover deterministic fixture parity, browser restart/repair,
transactional validation, locks, checksum invalidation, portable exports,
cleanup and interrupted cleanup, local provider/Anki services, real media clips,
HTTP ASR with delayed audio and chunk rebasing, second-ASR recovery, and the
GUI/native-engine handoff. The CLI acceptance test completes with an empty PATH;
its extraction fixture is a compiled Rust binary. Existing GUI accessibility
interaction tests continue to cover the approved controls and appearance.

CI runs formatting, warnings-as-errors linting, and tests across macOS, Windows,
and Linux. These workflows are definitions until run on those machines; local
verification does not constitute cross-platform release acceptance.

## Remaining release work

1. Supply licensed, platform-specific compiled Blitzer/FFmpeg/FFprobe/whisper.cpp
   builds and model artifacts, with pinned URLs and SHA-256 catalogs. The GUI
   currently reports availability and accepts a configuration; a fully guided
   catalog-based download/update screen remains to be connected.
2. Validate real local whisper.cpp and live hosted service compatibility against
   release artifacts. Existing tests use native media tools and synthetic HTTP
   servers; they do not consume paid accounts or validate model quality.
3. Import generated APKGs into an isolated real Anki collection, including custom
   schemas/assets and repeated import; exercise live AnkiConnect recovery.
4. Finish OS credential storage, embedded media/HTML preview, media cue editing,
   and complete keyboard/screen-reader acceptance.
5. Produce signed/notarized macOS distribution, Windows installer, and Linux
   application packaging. Verify cold installs on all supported platforms with
   no Python installed. Current bundles/archives/workflow artifacts are unsigned.
6. Review tool/model licenses and notices, dependency/security checks, minimum OS
   versions, updater trust/rollback, and installer uninstall behavior before a
   public release. No third-party native binaries are silently downloaded by a
   normal project run.
