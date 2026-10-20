# Rust port implementation plan

## Scope and technical foundation

Implement the organization in [architecture.md](architecture.md), subject to the
preservation rules and exceptions in [compatibility.md](compatibility.md).

Use typed Rust records with Serde serialization, TOML configuration, and JSON
job artifacts. Use Tokio for asynchronous execution and Reqwest for HTTP.
Keep blocking work away from the UI thread. Pin the toolchain and dependency
versions when creating the workspace and retain the application lockfile.

Upstream references:
[Tokio](https://docs.rs/tokio/latest/tokio/),
[Reqwest](https://docs.rs/reqwest/latest/reqwest/).

## Shared application contract

Expose typed operations for:

- Creating, inspecting, and listing jobs.
- Starting, pausing, resuming, and retrying processing.
- Preparing and submitting assisted-review exchanges.
- Applying direct edits and locating moved source files.
- Checking configuration and resource availability.
- Exporting results and previewing/applying cleanup.

Return structured job snapshots, progress events, pending interactions, and
actionable errors. Stable error codes must be separate from diagnostic text.

Keep existing CLI command names and exit-code meanings where the behavior still
applies. Add CLI access to assisted interactions and structured JSON output.
Use the same application operations in the desktop app.

### Job persistence and interruption

- Persist versioned job state with atomic manifest updates and a per-job writer
  lock.
- Verify checksums before reusing outputs or request caches.
- Invalidate dependent stages when relevant inputs change.
- Preserve accepted decisions and completed requests during retries.
- Opening a job must never automatically initiate paid requests.
- Pause stops scheduling new work and saves completed work. Define handling of
  in-flight requests and child processes explicitly; do not mark interrupted
  work complete.
- Resume from persisted state after closing or crashing.
- Preserve delivery receipts. Do not promise exactly-once remote execution when
  the provider cannot reconcile a lost response.

### Assisted exchanges

Persist job revision, stage, batch identity, expected response, and validation
status for each exchange. Generate prompts from the same task definitions used
for API processing.

Accept plain JSON or a single fenced JSON response. Give actionable feedback
for malformed, incomplete, duplicate, unknown, or stale responses. Preserve
valid items and prepare repair prompts for unresolved items. Changed source
inputs invalidate affected exchanges explicitly.

Completed and pending exchanges must survive restarting the app. Browser-chat
assistance requires no browser automation.

### Settings and credentials

Import supported Python TOML settings and report settings that require
attention. Keep the original configuration unchanged and use separate Rust
application storage. Resolve imported relative paths against the source
configuration file.

Desktop-entered keys use operating-system credential storage. If that facility
is unavailable, offer session-only credentials rather than silently persisting
plaintext. CLI environment variables remain supported.

## Implementation milestones

### A. Documentation and reference baseline

Create these planning documents, then inventory Python capabilities,
configuration options, tests, and intentional departures. Capture synthetic
fixtures and expected outputs from its current working tree using the procedure
in [compatibility.md](compatibility.md#reference-baseline).

**Gate:** every capability is classified as preserved, replaced, or excluded
with a reason. Fixture provenance identifies the working tree, not just HEAD.

### B. Shared contracts and simulated engine

Establish the workspace libraries, typed records, application operations,
integration traits, and simulated engine. Define job and interaction states.
Provide controllable scenarios for progress, waiting for input, failure,
interruption, and recovery.

**Gate:** simulations demonstrate success, review, pause, failure, retry, and
reopening without external services.

### C. GUI design and skeleton

Separately approve the framework, navigation, configuration controls, and
accessibility behavior. Build the desktop skeleton against the simulated engine.
This phase also determines minimum OS versions and installer formats.

**Gate:** a beginner can complete the proposed journey, including copy/paste
repair and recovery, before live processing is connected.

### D. Domain and storage

Port transcript/text handling, vocabulary mapping, review validation,
identities, configuration import, checksums, locking, and recovery.

Explicitly address Unicode character versus byte offsets, normalization,
Python-compatible identity serialization, millisecond rounding, and untimed
text records.

**Gate:** deterministic outputs match reference fixtures; interruption,
checksum, and downstream-invalidation tests pass.

### E. First working pipeline

Connect the CLI, Blitzer adapter, text input, direct/browser-assisted review,
and TSV export. Attach these working capabilities to the approved GUI.

**Gate:** text becomes reviewed vocabulary through either interface without
transcription or media dependencies.

### F. LLM automation

Port OpenRouter, DeepSeek, and compatible HTTP profiles, including per-stage
selection. Add proofreading, translation, ambiguity review, correction-only
audit, bounded item repair, and request caching.

Keep transport retries separate from content repair. Preserve provider options
such as response modes, reasoning controls, routing, and configured limits.

**Gate:** automatic and assisted answers pass the same validators. Transport
failures never enter content-repair loops. Successful items survive retries.

### G. Media and transcription

Add FFmpeg/FFprobe, imported SRT/ASR JSON, native transcription, hosted
transcription, custom HTTP, and re-listening.

**Gate:** recording-to-cards works with correct audio-stream offsets, validated
timing, clip boundaries, and recoverable chunk processing.

### H. Complete exports

Add APKG and AnkiConnect, note presets and customization, conditional templates,
assets, ordering, partial delivery, and explicit cleanup.

**Gate:** real Anki imports and repeated delivery preserve identities and
scheduling. Invalid or incomplete results cannot silently become complete
exports.

### I. Distribution and release

Package tools, resource acquisition, credentials, installers, CLI bundles,
diagnostics, and concise user documentation.

**Gate:** clean-machine acceptance succeeds on all supported platforms without
Python or terminal setup.

## Integration implementation defaults

### Blitzer

Retain the subprocess JSON boundary and mandatory extraction flags, including
frequency, lemmatization, context, and HTML highlighting. Disable prompts,
source preambles, history saving, and word-list mutations during processing.
Fingerprint the actual executable, selected configuration, language pack, and
active filtering inputs.

Develop against fixtures and optionally the existing Python executable. A
compatible compiled Blitzer is a release dependency; using Python Blitzer
during development does not satisfy release acceptance.

### Local transcription

Package a pinned `whisper.cpp` executable and acquire preconverted models through
Blitzline. Start with CPU support on all platforms and Metal acceleration on
macOS. End users must not run model-conversion scripts or build dependencies.

Use validated segment timing as the baseline. Treat experimental word timestamps
as distinct from WhisperX forced alignment; do not manufacture precise word
alignment when unavailable. Different recognition results are accepted.

Reference: [whisper.cpp](https://github.com/ggml-org/whisper.cpp).

### Hosted transcription and custom HTTP

Use Groq as the initial hosted-provider preset, with configurable model
selection and timestamped responses. Normalize segment and optional word timing
into the same records as other sources. Apply provider-specific upload limits,
bounded chunking, caching, and retry behavior.

Retain the Python project's custom HTTP worker contract separately, including
chunk-relative timestamps and restoration of the source timeline. Modal workers
may expose that contract through HTTP. Direct Modal function invocation and
SDK call-ID reconnection are excluded from v1.

Reference: [Groq transcription API](https://console.groq.com/docs/speech-to-text).

### Media

Ship controlled FFmpeg/FFprobe builds with required notices. Preserve selected
audio-stream handling, stream offsets, padding, duration limits, audio/video
clips, and verification of generated clips. Text jobs must not initialize or
require media tools.

### APKG and AnkiConnect

Implement an isolated Rust SQLite/ZIP writer for the existing legacy APKG
format. Use the Python writer and captured packages as references. Preserve
fields, conditional card generation, typed-answer templates, GUIDs, media
mapping, deck/model IDs, and ordering.

Test through real Anki imports as well as structural archive checks.
AnkiConnect delivery must retain model compatibility checks, opt-in template
updates, stable-identity lookup, skip/update behavior, scheduling preservation,
and recovery receipts.

References:
[reference writer](../../blitzline-py/blitzline/exports/package.py),
[genanki](https://github.com/kerrickstaley/genanki).

### Managed resources

Bundle executables. Acquire larger models and language packs through a resource
catalog with versions, checksums, progress, retry, and explicit installation.
Retain resource identities in jobs so replacement invalidates affected work.
Support preprovisioned local resources for offline use.

## Verification and release criteria

| Area | Required coverage |
| --- | --- |
| Domain parity | Unicode, normalization, rounding, source matching, text-only behavior, grouping, coverage, templates, identities |
| Recovery | Interrupted stages, response saving, clip creation, partial Anki delivery, corrupt caches, moved sources, changed settings |
| Assisted workflows | Stale replies, duplicate submissions, malformed/incomplete responses, repair, restart, valid-item preservation |
| Concurrency | Competing writers cannot corrupt a job |
| Interface parity | Equivalent CLI and desktop operations produce equivalent jobs and results |
| Exports | Parsed package contents, actual Anki imports, media playback, identity preservation, repeated delivery |
| Installation | No Python, manual dependency installation, or terminal setup on supported desktop targets |

Compare parsed outputs rather than nondeterministic archive bytes. Routine
tests use fake servers and controlled subprocess fixtures. Live transcription,
language-quality evaluation, paid API checks, and disposable-Anki acceptance
are separate, explicitly invoked checks.

The first binary matrix is macOS ARM64/x86-64, Windows x86-64, and Linux x86-64.
Minimum OS versions and installer formats are settled during GUI design.
GPU support beyond macOS Metal is subsequent work; CPU operation is required.

On each platform, a new user must be able to install Blitzline, obtain resources,
configure a key or choose browser assistance, select input, correct a failed
response, reopen unfinished work, and export usable cards without a terminal.

Completion means the entire supported workflow is Python-free, including
transcription, package generation, and dependency setup.
