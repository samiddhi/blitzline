# Blitzline CLI

This executable runs without Python, Cargo, or the desktop application.
Copy it to a folder of your choice. On macOS/Linux, retain executable permission.
You can also install this same binary from the desktop application's Settings.

Run `blitzline --help` for commands and `blitzline capabilities` for contracts.
Use `blitzline check --text-only` to check the compiled Blitzer dependency.

Text processing requires compiled Blitzer, or imported Blitzer JSON through
`--vocabulary-json`. Media processing also requires native FFmpeg/FFprobe;
local transcription requires whisper.cpp and a preconverted GGML model.
Tools can be next to the executable in `tools/`, in the app's Resources/tools,
in the managed resource directory, on PATH, or selected by configuration.
Interpreted Blitzer/ASR wrappers are rejected; Python is never installed.

Start with browser assistance:

```
blitzline run article.txt --text-only --language slv --method browser --output my-run
blitzline prompt my-run
blitzline submit my-run --file reply.json
blitzline resume my-run
blitzline export my-run --format apkg
```

A run waiting for your response exits with code 3. Copy the whole prompt into a
browser LLM chat, save its JSON response as reply.json, then submit it. Submission
only validates and saves; resume continues processing. Prompt IDs and revisions
protect accepted work from old replies. No API key is needed for browser review.

Keys for automatic processing come from configured environment-variable names.
Never put a key in TOML. Use `--config FILE` to select a configuration. Existing
Python jobs stay with blitzline-py; native jobs use a separate manifest format.

This is an unsigned development build. Release signing/notarization, bundled
tool/model catalogs, and clean-machine acceptance are separate release gates.
