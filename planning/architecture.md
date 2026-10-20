# Blitzline architecture

## Product goal

One shared Rust engine serves two complete interfaces: a CLI and a desktop
application. The desktop application handles setup, configuration, processing,
review, recovery, and export without requiring a terminal.

Keep the useful boundaries in the Python project between orchestration, domain
transformations, persistence, integrations, and exports. Make user interaction,
including browser-chat copy and paste, a first-class engine capability.

Ordinary use must not require editing TOML or JSON, finding intermediate files,
constructing prompts, managing executable paths, or installing Python.
Advanced controls remain available without overwhelming the initial workflow.

## Workspace organization

Use one Cargo workspace, initially with three libraries and two applications:

```text
crates/
  domain/                   Records, transformations, validation, identity
  engine/                   Workflow, application operations, integration traits
  adapters/                 Persistence and external integrations
apps/
  cli/                      blitzline executable
  desktop/                  Added after GUI design approval
assets/
  presets/                  Note definitions and presentation assets
tests/
  fixtures/                 Portable inputs and expected outputs
  acceptance/               End-to-end and cross-interface scenarios
distribution/               Packaging, tools, resource manifests
planning/                   Architecture, implementation, compatibility
```

The workspace and desktop skeleton now exist. The shared engine is simulated;
the full production adapter set and per-job artifact layout remain to be ported.
See [GUI acceptance](gui-acceptance.md) for the implemented boundaries.

| Responsibility | Owner |
| --- | --- |
| Transcripts, candidates, decisions, cards, matching, validation | Domain |
| Stage ordering, job lifecycle, resumability, pending interactions | Engine |
| Job files, locks, checksums, caches, receipts | Storage adapter |
| Preferences, language/deck profiles, settings import | Configuration adapter |
| API keys and environment-variable lookup | Credential adapter |
| Required tools, model downloads, language packs | Resource adapter |
| Blitzer, transcription, LLM APIs, FFmpeg/FFprobe | Integration adapters |
| TSV, APKG, AnkiConnect | Export adapters |
| Commands, flags, terminal output, scripting | CLI |
| Guided setup, visual configuration, review, recovery | Desktop application |
| Installers, dependency bundles, release verification | Distribution |

Dependency direction:

- Engine depends on domain and declares integration interfaces.
- Adapters implement those interfaces and depend on engine/domain.
- CLI and desktop compose engine with concrete adapters.
- Domain has no filesystem, network, process, or UI dependencies.
- Engine does not depend on concrete adapters or either interface.

The desktop application calls the engine directly. It must not interpret
terminal output or duplicate pipeline decisions. Run long-lived work away from
the UI thread. Split adapters into additional crates only when independent build
requirements justify doing so.

Blitzer stays behind a subprocess contract. Its implementation language must not
determine Blitzline's internal organization.

## Saved jobs

A job contains a recording or text source reference, the effective settings,
progress, and results. It owns:

- Source identity and references to required input files.
- Effective configuration and selected resource identities.
- Completed stage results and reusable intermediate work.
- Pending questions, edits, and browser-chat exchanges.
- Generated media, exports, delivery receipts, and diagnostics.

Use versioned job directories with a manifest, artifacts, pending interactions,
media, exports, and logs. Any application-wide job index is rebuildable; the job
directory is authoritative.

Keep application preferences and credentials separate from jobs. Language and
deck profiles provide reusable defaults, while each job records its actual
choices. Store credential references, not secrets, in job settings.

Model processing, waiting for input, paused, failed, and finished as explicit
states. A job waiting for a pasted response is proceeding normally.

Both interfaces can start, inspect, pause, resume, retry, submit input, and
export the same jobs. Recovery, checksum validation, downstream invalidation,
and duplicate-delivery protection belong to the engine.

## AI assistance

For each AI-assisted stage, support:

1. API processing, where Blitzline sends and receives the request.
2. Browser-chat assistance, where Blitzline prepares a prompt and accepts a
   pasted reply.
3. Direct human review, where suitable for the stage.

Optional stages can be disabled. A job can mix these methods across stages.

Separate task preparation, obtaining an answer, and answer validation.
API responses and pasted replies pass through the same semantic validators.
Browser assistance owns batching, instructions, progress, and repair requests.
Users should never need to construct prompts or edit intermediate JSON files.

Blitzline owns timestamps, source matching, highlights, media references, and
export formatting. Models supply constrained decisions or corrections; the
engine constructs final artifacts.

## Setup, accessibility, and recovery

- Offer sensible presets and plain-language controls. All supported pipeline
  settings must be accessible from the desktop app, with advanced controls
  available as needed.
- Allow users to enter and test keys. Use operating-system credential storage;
  retain environment-variable support for CLI users.
- Own bundled-tool discovery and guided model/language-resource installation.
- Return actionable errors such as locating a moved file, replacing a key, or
  correcting a response. Keep technical diagnostics separately accessible.
- Treat keyboard operation, readable sizing, screen-reader support, and clear
  language as GUI framework selection criteria.
- Require no Blitzline account or hosted Blitzline backend. External providers
  may require their own accounts when selected.

The Python-free requirement covers the complete supported workflow, including
transcription, resource acquisition, package generation, and installation.

## Delivery organization

Proceed through organization approval, port planning, GUI design, simulated GUI
prototype, incremental integration, and clean-machine verification.

The GUI prototype uses the intended application interface with a simulated
engine. It must demonstrate the complete beginner journey, invalid pasted
responses, and reopening unfinished work before live processing is connected.

Verification has three levels: domain/pipeline tests, shared CLI/GUI workflow
tests, and clean-install usability checks. Keep the Python implementation and
captured fixtures as behavioral references during the port.
