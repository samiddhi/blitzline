# Development and configuration

## Navigation

Follow `cli.run` → `core.run_pipeline` → `core.execute_pipeline`. The sequence
is explicit, numbered, and backed by `storage.RunStore.stage`. Each Python file
opens with its scope, included/excluded responsibilities, and starting function.
Pure transformations live in `transcript.py` and `vocabulary.py`; adapters under
`integrations/` and `exports/` own external effects. `review.py` owns bounded
batching/validation, not HTTP. `media.clip_plan` is pure; `extract_clip` owns I/O.

Keep guard clauses and simple named functions. Tests, fixtures, private helpers,
and methods need docstrings. `tests/test_storage_cli.py` checks every Python
module's scope header and function docstrings. Historical `ref/tsvaudio2` is
neither imported nor considered maintained production code.

## Settings

Selection: `--config` → `BLITZLINE_CONFIG` → platform user config directory's
`config.toml`. `XDG_CONFIG_HOME/blitzline/config.toml` is used when set.
Missing default config uses fresh defaults; a missing explicitly selected file
is an error. Precedence is CLI choices → selected TOML → defaults. Paths inside
TOML resolve relative to that file. Resume uses the saved effective settings
unless `--config` selects new ones; non-None CLI flags override either source.

The example lists the common settings. Additional supported values are
`asr.timeout` (7200 seconds), `blitzer.timeout` (120), and `media.timeout` (300).
The configured WhisperX model/alignment models may download on first use;
Blitzline does not install ASR, replace your model choice, or install packs.
Use an already provisioned ASR environment for offline operation.

### Providers

Choose `llm.default` or a named profile separately for `llm.proofread`,
`llm.translate`, and `llm.review`. No default profile means manual review.
Proofread/translate are off by default; explicitly enabling either requires
its profile. Profiles contain provider, model, key environment-variable name,
optional base URL, response mode, temperature, max_tokens, timeout, and retries.
Literal key fields and credentials in URLs are rejected.

- `openrouter`: `https://openrouter.ai/api/v1`, `OPENROUTER_API_KEY`. It sends
  the app title and requires provider support for requested JSON parameters.
  `json_schema` uses the exact closed response schema; choose a compatible model.
- `deepseek`: `https://api.deepseek.com`, `DEEPSEEK_API_KEY`. Default mode is
  `json_object` with explicit JSON instructions.
- `compatible`: explicitly set your server's base URL and model. The adapter
  appends `/chat/completions`. Set the named key environment variable even if
  a local server accepts any placeholder value.

All modes are parsed and semantically checked locally. `text` omits the API
response-format parameter but still requires a JSON object. Responses finishing
with `length`, refusal, malformed JSON, or missing/extra IDs cannot complete a
stage. Requests use a finite timeout, size bound, and transient retry count.
Schema repair is limited by `llm.repair_attempts`; accepted items are retained
while missing/invalid items are re-requested with competing candidate evidence.

Related lemma candidates remain in the same batch. An exceptionally large
related group raises an actionable limit error rather than splitting competing
lemmas. `llm.max_chars` bounds serialized batch data, not tokenizer counts;
choose limits below the selected model's context window. `max_tokens` separately
bounds generated output. Neighbouring cues accompany translation/proofreading.
No audio-capable API is presumed: `asr.relisten=true` makes a second ASR pass on
questionable audio and passes that text as evidence, preserving source timing.

Protocol references: [OpenRouter requests](https://openrouter.ai/docs/api/reference/overview),
[OpenRouter structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs),
[DeepSeek JSON mode](https://api-docs.deepseek.com/guides/json_mode/),
[WhisperX](https://github.com/m-bain/whisperX), and
[genanki package/media/GUID API](https://github.com/kerrickstaley/genanki).

## Records and review

ASR input is a JSON object with `segments`, each containing text/start/end in
seconds and optionally aligned `words` with word/start/end. Times must be finite,
nonnegative, within the source, and in order; overlaps are allowed. Missing word
times fall back to whole-segment timing. Aligned words permit punctuation/pause/
length cue splits. Without alignment, segments retain their full text and timing.
SRT imports support BOM and multiline text and reject malformed timing blocks.
For new transcription, FFmpeg extracts the same selected audio stream used by
clipping to a temporary mono 16 kHz WAV. Any stream delay relative to the source
origin is recorded as `blitzline_audio_offset_ms` and restored during normalization.
Imported ASR JSON is assumed to use source-relative timing unless it explicitly
provides that offset. Imported SRT timestamps are already source-relative.
`asr.max_chars_per_second` and `max_cue_ms` also produce a nonblocking
`01-readability.json` report; text is never shortened to satisfy these targets.

Internally all times are integer milliseconds, and all offsets are Python Unicode
character offsets. SRT is generated locally, never taken from the ASR writer.
Canonical plaintext joins whitespace-normalized cues with spaces and stores a
cue span map. Whitespace normalization is explicit; punctuation and words remain
unchanged. Corrected cues lose old word alignment and use cue timing.

Blitzer is always invoked with `--freq --lemmatize --context --bold html`, JSON,
and no prompt/source preamble, known updates, or history saving. Other defaults
remain Blitzer's policy. `base` cannot support mandatory lemmatization: use a
real three-letter language pack. Blitzer output includes plain contexts and
highlight offsets, even with HTML bold selected. Blitzline escapes source text
and inserts markup itself.

Context matching is exact and case-sensitive. A sentence can span many cues;
there is no 15-cue ceiling. Repeated identical sentences map to the earliest exact
occurrence, with every match position saved. Blitzer supplies limited distinct
examples, not every surviving occurrence. The card Frequency is its original
candidate count, which can overcount ambiguous lemmas before semantic review.
Two examples are allowed, but one distinct example is sufficient.

A manual review has exactly this shape (use actual IDs from the run):

```json
{
  "decisions": [
    {
      "candidate_id": "candidate-ACTUAL_ID",
      "decision": "keep",
      "reason": "",
      "english": "context-specific meaning",
      "context_ids": ["candidate-ACTUAL_ID-context-1"],
      "notes": ""
    }
  ]
}
```

Every candidate needs exactly one `keep`, `reject`, or `needs-review` decision.
Kept items need English and one or two valid owned context IDs. Rejected or
uncertain items need a reason. Notes are only learner-facing cognates/false
friends. The schema cannot change the term, frequency, examples, or timing.
`review-template.json` always reflects current candidates; `review.json` is
created once and preserved so your edits survive. If candidates changed, base
new edits on the updated template, not an old file with different IDs.

## Artifacts and recovery

Each run has `manifest.json`, numbered stage JSON in `artifacts/`, subtitle/text
artifacts, verified `media/`, per-format `exports/`, `logs/`, and private `.cache/`
for individual model responses. The original recording stays at its original
path; the run stores a fingerprint and imported raw transcript, not a second
copy of the potentially large recording. Keep the original and selected input
files available when resuming.

Stage reuse checks input hashes and every committed output checksum, never just
file existence. Config/known/exclusion/pack contents and CLI version participate
in Blitzer extraction keys. Downstream stages are invalidated before changed
upstream work runs. Individual LLM requests have profile/prompt/schema/data cache
keys and checksums; no bearer keys are saved. Successful stages survive failure.
OS locks prevent concurrent writers; interruption/crashes release the lock.

Use `--stop-after transcript|proofread|translate|vocabulary|review|media|export`
to stop intentionally. If proofreading remains uncertain, inspect its report,
correct the SRT, and resume with `--srt corrected.srt --no-proofread`. Translation
errors require fixing configuration/retrying or `--no-translate`. Vocabulary
errors require edited review JSON or new provider settings. Clip failures require
valid shorter examples or adjusted media settings. Unresolved candidates and
clips block export unless `--allow-partial` explicitly allows accepted cards;
reports retain all omitted IDs and reasons.

Exit codes: 0 complete/intentional stop, 1 failed external/file operation,
2 invalid arguments/configuration, 3 needs review. Progress goes to stderr.

## Exports

Ten fields, with the original six first: Word, English, Sentence1, Sentence2,
Notes, Frequency, Media1, Media2, Source, BlitzlineId. One vocabulary card per
note. Media uses `[sound:filename]` for both MP3 audio and MP4 video; playback
behavior must be checked in the Anki client you use. UTF-8 text is escaped before
HTML rendering. TSV comments describe separator/HTML/columns; TSV has a real
writer and round-trip check, plus referenced media and import instructions.

Templates/CSS are configurable with `export.front_template`, `back_template`,
and `css`. These apply to APKG and newly created AnkiConnect note types;
existing Anki templates are preserved, so use a new model name to replace them.

APKG uses the optional `genanki` extra, shared templates, deterministic model/deck
IDs, and source/language/NFC-lemma note GUIDs. Reimporting the same source/lemma
keeps identity even after English or notes change. Another recording creates a
new identity. TSV normally matches duplicates by Word; different recordings can
collide under Anki's TSV import rules, so read its import guide.

Direct delivery uses AnkiConnect API v6. It checks model fields before mutations,
uploads referenced media, finds a stable identity before each note, skips existing
notes by default, or updates only shared fields with `export.existing="update"`.
It never edits the live collection database or scheduling. Per-note receipts
record partial delivery; a lost add-note response can be retried without adding
a duplicate. No live Anki action happens unless `anki` is the selected export.

## Verification

Run `python -m pytest -q` after installing `.[dev]`. Tests use an isolated temporary
SQLite Blitzer pack, generated four-second video/audio, and deterministic fake
model/Anki replies. They exercise real Blitzer, FFmpeg, SRT translation timing,
manual and automated review, cache changes, Unicode, empty results, malformed
API responses, JSON repair, and interrupted delivery. The APKG test runs when
`genanki` is installed. No test uses personal recordings or credentials.

Separate live acceptance checks still require a provisioned WhisperX model,
a selected paid/API model, and a disposable Anki profile. Generate one short
foreign-language run, inspect subtitle corrections/translations, listen to both
examples, import APKG/TSV, verify MP4 playback, and retry Anki delivery. Protocol
and generated-file tests cannot establish transcription/translation quality or
client-specific playback by themselves.
