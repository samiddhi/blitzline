# Get started

1. Install Python 3.11+, FFmpeg, `bltzr`, and a Blitzer pack for your language.
   Install WhisperX too if you need transcription; existing SRT or ASR JSON
   works without it. Point `asr.executable` to its executable if it uses another
   environment.
2. In this checkout, install Blitzline:
   `python3 -m pip install -e '.[apkg]'` (prefer a virtual environment).
3. Copy [config.example.toml](config.example.toml). Select your OpenRouter
   model and set `OPENROUTER_API_KEY` in your environment. You can instead
   select DeepSeek or a custom endpoint. Keep your keys out of the file.
4. Set your Blitzer config/known-word list if its normal defaults are unsuitable.
   Pick `audio` or `video` clips and `tsv`, `apkg`, or `anki` delivery.
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
and recovery options.
