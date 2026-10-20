# Blitzline — Rust desktop prototype

A working GUI skeleton for turning recordings and text into study cards.
The shared Rust engine is currently a **deterministic simulation**, so the
interface can be evaluated before connecting the real pipeline.

Read the [full GUI specification](planning/gui.md),
[architecture](planning/architecture.md), and
[implementation/acceptance status](planning/gui-acceptance.md).

The macOS build has a real AppKit toolbar and menus, a light/dark toolbar
switch, and system typography. Content uses the shared Rust/egui renderer.
See the [macOS design revision](planning/macos-design.md) for the native boundary.

## Try it

Build a double-clickable macOS application:

```sh
sh distribution/package-macos.sh --debug
```

Open `target/Blitzline Prototype.app` in Finder. The built app does not need
Python, Cargo, or a terminal to run.

For development on macOS, Windows, or Linux:

```sh
cargo run --locked -p blitzline-desktop
```

Choose **Try an example**, then **Go to current step** when vocabulary is ready.
Select **Open chat assistant** to open its separate window. Expand
**Try a sample reply — prototype only**, fill a
sample reply, and select **Check answer**. Complete the second batch, prepare
the study cards, preview, and continue to export. An incomplete sample reply
demonstrates repair without losing accepted answers.

You may also copy the prepared prompt to a real browser chat and paste its
answer back. Opening a website never sends a prompt automatically.

For isolated state:

```sh
cargo run --locked -p blitzline-desktop -- --data-dir .prototype
```

`--scenario assistant|cards|export|setup|projects|settings` seeds a demonstration
view. Use an isolated data directory for testing. `--narrow` starts at 1024×768.

## What works

- Native Rust windows, guided setup, separate projects, and stage navigation.
- Project/settings/draft persistence with atomic saves and exclusive ownership.
- Guided copy/paste, validation, partial repair, stale-answer rejection,
  smaller sample batches, and direct review.
- Pause/resume, queue progression, simulated failures, and recovery.
- Card preview/export gating and actual sample TSV output.
- Searchable settings, profiles, managed word lists, session-only credential
  entry, and basic Python deck-profile import.
- A detachable assistant and separate template editor with safe text preview.
- A CLI using the same application operations:

```sh
cargo run --locked -p blitzline-cli -- --data-dir .prototype-cli example
cargo run --locked -p blitzline-cli -- --data-dir .prototype-cli advance
cargo run --locked -p blitzline-cli -- --data-dir .prototype-cli list
```

GUI and CLI cannot write the same prototype workspace simultaneously.

## Simulation boundaries

Transcription, Blitzer extraction, paid AI requests, audio/video playback, model
downloads, APKG generation, and Anki delivery are **not connected**.
The UI labels those operations. Vocabulary is bundled demonstration data;
the app never claims it analyzed a user's file or delivered a real deck.

The sample TSV is real but contains demonstration words. APKG export records
only a simulated receipt. Keys remain in memory and are never sent or saved.
Production OS credential storage remains pending.

Template editing saves front/back/CSS and previews substitutions as plain text;
it never executes HTML/JavaScript. Import applies language/deck profiles and
reports unsupported settings.

Prototype storage is one versioned workspace rather than production job folders.
It uses the separate `Blitzline Prototype` application data directory and never
modifies Python Blitzline or Blitzer data.

## Development

Rust 1.93.1 was used locally. The prototype uses egui/eframe 0.33.3 with AccessKit.
This is not a final production toolkit commitment: full assistive-technology and
playback checks are still required.

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
```

The workspace separates domain values, engine operations, storage/import
adapters, CLI, and desktop UI. Tests use synthetic data and fake processing.
No credentials, personal recordings, or paid services are needed.

The Python reference is `../blitzline-py`, including its uncommitted changes.
Read [compatibility](planning/compatibility.md) before porting real behavior.
