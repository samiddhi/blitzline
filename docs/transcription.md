# Local and remote transcription

Blitzline can transcribe locally with WhisperX, invoke a deployed Modal function,
or send audio to an HTTP worker. Select `asr.backend` in TOML, or override it with
`--asr-backend local|modal|http` on `run` or `resume`. SRT/ASR JSON imports bypass
all transcription backends. For multiple languages, select a configured
`--deck-profile`; its source language is mapped to the ASR language code for both
local and remote recognition. Explicit `--language` must match the selected
profile. See [language/deck/card-type configuration](anki.md#multiple-languages-and-explicit-deck-profiles).
`blitzline check` reports the selected backend's
configuration without contacting it.

All backends feed the same segment/word validation, cue splitting, source timing,
and subtitle generation. FFmpeg remains local: it extracts the selected source
audio stream as mono 16kHz PCM WAV. Remote backends upload chunks (600 seconds by
default), cache each validated result, and rebase segment and word timestamps
into the source timeline. Remote audio stays in temporary local files. Only
transcripts, cache checksums, and Modal call IDs persist in the run folder.

## Modal

Install the optional SDK, authenticate, and deploy the supplied worker:

```sh
python -m pip install -e '.[modal]'
modal setup
modal deploy examples/modal_asr.py
```

The example uses faster-whisper on an L4 GPU, includes the CUDA/cuDNN runtime,
and keeps downloaded models in a Modal Volume. It returns segment and word
transcription timestamps; these are faster-whisper timestamps, rather than a
separate WhisperX forced-alignment pass. Change the worker if you prefer
WhisperX alignment or another recognizer. GPU/image/volume choices live in the
worker deployment; local `asr.device` and `compute_type` apply to local WhisperX.

```toml
[asr]
backend = "modal"
model = "small" # Or another model supported by your deployed recognizer.
modal_app = "blitzline-asr"
modal_function = "transcribe"
# modal_environment = "production" # Omit to use the SDK's selected environment.
remote_compute_type = "float16"
chunk_seconds = 600
timeout = 7200

[asr.options]
beam_size = 5
vad_filter = true
```

Blitzline looks up the deployed function using the
[Modal function lookup API](https://modal.com/docs/guide/trigger-deployed-functions).
Each invocation uses this keyword contract:

```python
transcribe(
    audio=wav_bytes,
    language="sl",
    model="small",
    compute_type="float16",
    options={"beam_size": 5},
)
```

Any deployed function implementing that contract can be selected. Credentials
come from the Modal SDK's normal setup or environment, not the TOML or manifest.
Blitzline does not deploy workers automatically.

A spawned job's call ID is saved before waiting. If the client times out or is
interrupted, resume reconnects to the same job using
[FunctionCall.get](https://modal.com/docs/sdk/py/latest/FunctionCall).
Completed chunk results are reused without another GPU invocation. If a worker
job failed permanently or its remote result expired, fix the worker/config and
remove only the affected `.cache/asr/HASH.modal.json` receipt before retrying;
completed chunk results can remain. Modal errors omit remote exception text;
inspect the worker's Modal logs for diagnostics. The small gap between submitting
a job and saving its call ID cannot guarantee exactly-once submission after a
hard process crash.

## HTTP workers and other hosts

HTTP uses a provider-neutral worker contract. Deploy an adapter implementing it
on your preferred host, such as your own server, a GPU host, or a Modal web
endpoint. Existing provider APIs with different request/response formats need
an adapter; this is not a direct client for every vendor's speech API.

```toml
[asr]
backend = "http"
endpoint = "https://your-worker.example/transcribe"
api_key_env = "TRANSCRIPTION_API_KEY" # Omit for an unauthenticated worker.
model = "small"
remote_compute_type = "float16"
chunk_seconds = 600
timeout = 7200
retries = 2

[asr.options]
beam_size = 5
```

Blitzline POSTs JSON to the exact configured endpoint:

```json
{
  "audio": "BASE64_ENCODED_PCM_WAV",
  "filename": "audio.wav",
  "language": "sl",
  "model": "small",
  "compute_type": "float16",
  "options": {"beam_size": 5}
}
```

If configured, the key is read from `api_key_env` and sent as
`Authorization: Bearer ...`. Credentials and response bodies are omitted from
transport errors. A deterministic `Idempotency-Key` header identifies each
chunk/request; workers should honor it to avoid repeat processing when HTTP
responses are lost. HTTP 408/429/5xx and connection failures have bounded retries
(`retries = 2` means at most three attempts). Timeout applies per request.
Completed chunks survive later failures. There is no asynchronous polling
contract: the worker must return its transcript in the request's response.

## Worker responses and timing

Modal and HTTP must return a JSON-serializable object with this shape:

```json
{
  "segments": [
    {
      "start": 0.1,
      "end": 1.2,
      "text": "Primer stavka.",
      "words": [
        {"word": "Primer", "start": 0.1, "end": 0.6},
        {"word": "stavka.", "start": 0.6, "end": 1.2}
      ]
    }
  ]
}
```

Times are seconds relative to the supplied chunk, not the original recording.
Word timing is optional. All nonempty segments need valid start/end times
inside the chunk; `{"segments": []}` is valid for silence. Plain text alone is
insufficient for timed subtitles. Remote source offsets are ignored; Blitzline
restores the locally measured audio-stream offset itself. HTTP responses use
the shared 4MiB response bound.

`chunk_seconds` is between 1 and 3600. Short chunks reduce upload/memory size;
longer chunks provide more language context. Chunk boundaries have no overlap
and may cut across speech, so choose a suitable size for your recordings and
worker limits. Each default 600-second PCM upload is approximately 19.2MB
before JSON's base64 expansion. Chunks run sequentially. Switching models,
backend, deployment names/environment, audio, language, or decoding options selects
a fresh cache. Redeploying different code under the same function name is not
automatically detected; use a fresh run/cache when changing the worker's behavior.
Cached result checksums are verified on reuse.

`asr.relisten = true` uses the selected backend again for short uncertain cue
intervals. The second transcript is proofreading evidence; it does not replace
source timing. Proofreading itself still requires its configured LLM profile.

For local operation, keep `backend = "local"` and configure `executable`, `model`,
`device`, and `compute_type` as before. A separate WhisperX environment is
supported through its executable path. See the
[faster-whisper GPU requirements](https://github.com/SYSTRAN/faster-whisper#gpu)
for the supplied worker's runtime libraries.
