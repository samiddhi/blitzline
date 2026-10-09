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

The command prints the run folder. `artifacts/source.srt` is the source-language
subtitle; `artifacts/english.srt` is created when translation is enabled.
Your cards and media are under `exports/`. Import `.apkg` through Anki's Import
command, or follow `exports/tsv/IMPORT.md` for TSV.

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
