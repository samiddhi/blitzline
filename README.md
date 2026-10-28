# Blitzline

A Rust pipeline and native desktop application for turning text and recordings
into reviewed language-learning cards. The installed binaries need **no Python
or other language runtime**.

The desktop retains its native macOS toolbar, light/dark appearance, guided
setup, and separate chat assistant. Normal projects use the real pipeline;
**Try an example** uses explicitly marked demonstration data.

## Application distribution

End users should receive a prebuilt macOS application, Windows installer, or
Linux desktop package. They should not build from source, install Rust/Python,
or open a terminal. Current local app bundles and CI artifacts are development
builds; public installers and signed releases are still in progress.

Normal app data lives in the user's OS application-data directory, outside the
repository and installed app. Source files stay where the user selected them.
The app stores references; pasted text and generated artifacts live in its run
folders. `--data-dir` is an explicit development override.

## Developer builds

Build a double-clickable macOS app, including its standalone CLI:

```sh
sh distribution/package-macos.sh --debug
```

Open `target/Blitzline.app`. In Settings → Import & diagnostics, choose
**Install command-line tool…** to copy the bundled CLI to your preferred folder.
It works independently of the app. Omit `--debug` for a release build.

For CLI-only installation, build `cargo build --release --locked -p blitzline-cli
--bin blitzline` and copy `target/release/blitzline` to your preferred executable
folder. Unix CLI archives can be built with `sh distribution/package-cli.sh`.
Windows builds produce `blitzline.exe`. Rust is needed on the build machine only.
The manual distribution workflow builds unsigned platform artifacts.

Native extraction needs a **compiled Blitzer** distribution that implements the
existing JSON/argument contract. Its separate port is not included here. Media
also needs FFmpeg/FFprobe; local speech recognition needs whisper.cpp and a
preconverted GGML model. Executables can be bundled, installed as managed
resources, found on PATH, or configured explicitly. Python wrappers are rejected.

Until a compiled Blitzer build is available, `--vocabulary-json FILE` imports
its documented JSON output and exercises the remaining real pipeline. This is
also useful for reproducible workflows.

## Browser assistance, without an API key

```sh
blitzline run article.txt --text-only --language slv --method browser --output my-run
blitzline prompt my-run
blitzline submit my-run --file reply.json
blitzline resume my-run
blitzline export my-run --format apkg
```

Copy the whole prompt into a browser chat, then save/paste the whole JSON reply.
Exit code **3** means the run is waiting for review. Submission saves validated
answers without dispatching the next request. Resume continues. Partial answers
retain valid items; follow-up prompts request the remainder. Old exchange IDs or
revisions cannot overwrite newer work.

The GUI provides the same copy/paste exchange in its chat assistant. You can
also type meanings and include/exclude words directly. Preview the current
cards before exporting. Exported files live in the project's run folder;
**Show exported files** opens that folder in your system file manager.

## Automatic processing and recordings

Select a configuration with `--config FILE`, `BLITZLINE_CONFIG`, or the default
application config directory. The GUI can select one in **Tools & downloads**.
A minimal configuration for automatic review:

```toml
[run]
method = "api"

[llm]
default = "reviewer"

[profiles.reviewer]
provider = "openrouter"
model = "YOUR_MODEL_ID"
api_key_env = "OPENROUTER_API_KEY"
```

Keys come from environment variables; desktop-entered keys are session-only.
OpenRouter, DeepSeek, and compatible HTTP profiles support configured response
modes, routing, reasoning, retries, and request caches. A profile's `parallel`
(1–16, default 1) sends that many requests of a stage at once; answers are still
applied in order. Automatic service use
requires the user's processing choice and may incur charges.

```sh
blitzline run recording.mp4 --language slv --method browser --output recording-run
blitzline run recording.mp4 --language slv --srt corrected.srt --output subtitle-run
blitzline resume recording-run --review decisions.json
blitzline export recording-run --format tsv
blitzline export recording-run --format anki
blitzline check --text-only
```

Transcription supports native whisper.cpp, Groq, any HTTP ASR service you run or host ([protocol](docs/asr-worker-protocol.md)), SRT, and
ASR JSON. Optional proofreading, translation, second-ASR evidence for uncertain
corrections, ambiguity review, and correction-only audit share saved tasks and
validators. Text stays untimed and never requires media tools.

Runs use locked folders, atomic manifests, checksummed artifacts, stable Anki
identities, bounded requests, explicit partial export, and per-note AnkiConnect
receipts. Cleanup previews first; `clean RUN --apply` requires verified portable
exports and makes working data nonresumable. Original source files are retained.
Existing Python jobs remain with `../blitzline-py`.

## Development and status

Development uses Rust 1.93.1, a committed Cargo.lock, bundled SQLite, and Rust TLS.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

HTTP tests use local fake services. Media tests use native FFmpeg/FFprobe when
available; set `BLITZLINE_REQUIRE_MEDIA_TESTS=1` to require them. No test needs real
credentials, paid services, or personal recordings. Reference fixture generation
used the Python development checkout; installed applications and Rust tests do
not invoke it. The old simulation CLI is separately named `blitzline-demo`.

See the [production UI audit](planning/production-ui-audit.md),
[implementation status](planning/implementation-status.md),
[architecture](planning/architecture.md), [port plan](planning/rust-port.md),
[compatibility](planning/compatibility.md), and [full GUI specification](planning/gui.md).

This is an unsigned development build. Fully managed tool/model catalogs,
signing/notarization, real-Anki import acceptance, Windows/Linux clean-machine
checks, embedded media playback/HTML preview, and production accessibility
acceptance remain release gates. The GUI currently opens recordings in the
system media player and previews card content as plain text. It supports whole
text-document corrections; correcting media cue timing uses imported subtitles
in a new project.
