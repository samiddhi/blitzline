# Blitzline

Turn a foreign-language audio/video file into subtitles and Anki vocabulary
with matching audio or video examples. Blitzline uses WhisperX for transcription,
`bltzr` for unknown-word extraction, and an optional LLM for proofreading,
English subtitles, and lemma review.

**LLM providers:** OpenRouter, DeepSeek, and custom chat-compatible endpoints.
Each stage can use a different provider/model. Internally, responses are validated
JSON; Blitzline generates timestamps, bold examples, media references, and TSV.

Start with the [short setup guide](docs/setup.md) and
[example configuration](docs/config.example.toml).

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -e '.[apkg]'
.venv/bin/blitzline check --config docs/config.example.toml
.venv/bin/blitzline run recording.mp4 --language slv --config docs/config.example.toml
```

Install FFmpeg and Blitzer with your language pack separately. Transcribe locally
with WhisperX or [offload to Modal or an HTTP worker](docs/transcription.md).
Choose your model and set its API key before using the example config.
WhisperX can run in another environment via `asr.executable`.

[Blitzer](https://github.com/samiddhi/blitzer-py/) can be installed from pip with:

```sh
pip install bltzr
```

Use a Blitzer build whose `bltzr blitz --help` lists
`--skip-exact-words-file` and `--skip-word-families-file` for the new lists.
If working from the updated Blitzer checkout, `blitzer.executable` can point
to its `.venv/bin/bltzr`.

Choose [which words to skip](docs/word-lists.md): **exact words (word form)**
skip only listed spellings, while **word families (lexeme/lemma)** skip a word
and its dictionary-connected forms. For **to be**, an exact list containing
**be** and **am** leaves **are** counted; a family list containing **be** skips
its recognized forms. Blitzline uses your Blitzer lists automatically, or you
can override either file in the `[blitzer]` section of your Blitzline config.

Already have subtitles? Skip transcription:

```sh
blitzline run recording.mp4 --language slv --srt recording.srt --config my-config.toml
# Or: --asr-json whisper-output.json
```

Without an LLM, the run stops with an editable `artifacts/review.json` containing
one decision per candidate. Fill its meanings and decisions using
`artifacts/04-vocabulary.json`, then continue:

```sh
blitzline resume RUN_DIR --review RUN_DIR/artifacts/review.json
blitzline export RUN_DIR --format apkg
# Direct delivery requires Anki running with AnkiConnect:
blitzline export RUN_DIR --format anki
```

The default [Anki note preset](docs/anki.md) matches `ref/example_note.apkg`:
clickable sentence cards, example audio, and optional typed recall. Fields,
mappings, multiple templates, CSS/JavaScript, tags, and assets are configurable.
The legacy format remains available. Configure a language and card types for each
[deck profile](docs/anki.md#multiple-languages-and-explicit-deck-profiles), then use
`--deck-profile slovenian` to select its deck and infer its source language. One
configuration supports multiple languages and decks.

Use `--clips video` for video examples, `--translate` for English subtitles, and
`--proofread` for transcript review. Optional `asr.relisten` performs a second ASR
pass on questionable audio; a text-only model is never presented as listening.
Unresolved items stop delivery; `--allow-partial` explicitly exports accepted
cards and keeps a report of omissions.

Runs contain `artifacts/`, `media/`, `exports/`, `logs/`, and `manifest.json`.
The command prints their location. Resume reuses checksum-valid stages and
invalidates downstream work when transcript/config/skip-list inputs change.
Blitzer's skip lists and history are never updated by this pipeline.

The original requirements and prompt are retained in [the vision](docs/vision.md).
Publish releases with `make release`; see the [one-time GitHub/PyPI setup and
release steps](maintenance/RELEASING.md). The helper bumps the patch version and
publishes a GitHub release, which triggers the PyPI workflow.

See [the implementation checklist](docs/plan.md) and
[developer/configuration guide](docs/development.md) for stage contracts,
limitations, and verification. Every Python module documents its scope and
navigation; domain transformations are functions independent of I/O.

```sh
python -m pip install -e '.[dev]'
python -m pytest -q
python -m ruff check blitzline tests
python -m ruff format --check blitzline tests
```
