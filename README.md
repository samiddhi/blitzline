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

Install FFmpeg, Blitzer with your language pack, and WhisperX separately.
Choose your model and set its API key before using the example config.
WhisperX can run in another environment via `asr.executable`.

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

Use `--clips video` for video examples, `--translate` for English subtitles, and
`--proofread` for transcript review. Optional `asr.relisten` performs a second ASR
pass on questionable audio; a text-only model is never presented as listening.
Unresolved items stop delivery; `--allow-partial` explicitly exports accepted
cards and keeps a report of omissions.

Runs contain `artifacts/`, `media/`, `exports/`, `logs/`, and `manifest.json`.
The command prints their location. Resume reuses checksum-valid stages and
invalidates downstream work when transcript/config/known-list inputs change.
Blitzer's known lists and history are never updated by this pipeline.

The original requirements and prompt are retained in [the vision](docs/vision.md).
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
