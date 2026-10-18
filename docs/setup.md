# Get started

1. Install Python 3.11+, FFmpeg, `bltzr`, and a Blitzer pack for your language.
   For transcription, install WhisperX or configure [Modal/HTTP offloading](transcription.md).
   Existing SRT or ASR JSON works without a transcription backend. For local
   WhisperX in another environment, configure `asr.executable`.
2. In this checkout, install Blitzline:
   `python3 -m pip install -e '.[apkg]'` (prefer a virtual environment).
3. Copy [config.example.toml](config.example.toml). Select your OpenRouter
   model and set `OPENROUTER_API_KEY` in your environment. You can instead
   select DeepSeek or a custom endpoint. Keep your keys out of the file.
4. Choose [which words to skip](word-lists.md) using Blitzer's exact-word list
   (word form) and word-family list (lexeme/lemma). Blitzline inherits the lists
   from `~/.config/bltzr/bltzr.toml`, or you can override either in `[blitzer]`.
   Use a Blitzer build with the new skip-file options in `bltzr blitz --help`.
   Configure a `[decks.NAME]` profile with its three-letter source language,
   destination deck, and `card_types` for each language you study. The example
   includes Slovenian and Polish. Pick `audio` or `video` clips and `tsv`, `apkg`,
   or `anki` delivery. Use `--deck-profile NAME` instead of `--language` to infer
   the language from a profile.
5. Check: `blitzline check --config YOUR_CONFIG`.
   Run: `blitzline run recording.mp4 --language slv --config YOUR_CONFIG`.

For a regular installed CLI, place the configuration at
`$XDG_CONFIG_HOME/blitzline/config.toml` and omit `--config`. If
`XDG_CONFIG_HOME` is unset, Blitzline uses the platform's user config directory.
An absolute `XDG_DATA_HOME` puts new runs under
`$XDG_DATA_HOME/blitzline/runs`; otherwise the platform user data directory is
used. Both XDG variables work on macOS as well. `--config` and `--output` still
override these defaults, and paths inside TOML resolve relative to the TOML
file. Keep API keys in environment variables, not in TOML.

The command prints the run folder. `artifacts/source.srt` is the source-language
subtitle; `artifacts/english.srt` is created when translation is enabled.
Your cards and media are under `exports/`. Import `.apkg` through Anki's Import
command, or follow `exports/tsv/IMPORT.md` for TSV.
Once you have inspected the imported deck, preview intermediate-file cleanup
with `blitzline clean RUN_DIR`. Add `--apply` only when you want to keep just the
portable export and a small receipt. This removes review JSON, request caches,
logs, and working clips; the run cannot be resumed afterward.

For direct delivery, keep Anki open with [AnkiConnect](https://ankiweb.net/shared/info/2055492159)
and choose `--format anki`. Blitzline uses the configured deck and note type.

If the run needs review, inspect its report and `artifacts/04-vocabulary.json`.
Edit `artifacts/review.json` and run
`blitzline resume RUN_DIR --review RUN_DIR/artifacts/review.json`.
Without an LLM, this manual path is the normal workflow.

Use `--srt FILE` or `--asr-json FILE` to skip transcription, `--clips video`
for video examples, and `--translate`/`--proofread` to enable those optional
stages. The [developer guide](development.md) explains additional settings
and recovery options. [Anki customization](anki.md) covers the default example
note format, alternate fields/templates/styles, and the legacy preset.
