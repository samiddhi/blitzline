# ASR service protocol

Set `asr.backend = "http"` and `asr.endpoint` to use any speech-recognition service that speaks this
protocol: a Whisper model you run yourself, an equivalent model, or a hosted API behind a small adapter.
Examples: [examples/asr-workers](../examples/asr-workers).

## Request

`POST <asr.endpoint>`, JSON body, with `Idempotency-Key` and, if `asr.api_key_env` names an environment
variable, `Authorization: Bearer <value>`:

```json
{"audio": "<base64 WAV, 16 kHz mono>", "filename": "audio.wav", "language": "slv",
 "model": "<asr.model>", "compute_type": "<asr.remote_compute_type>", "options": {}}
```

`language` is a three-letter code (may be empty). `model`, `compute_type` and `options` are passed
through from the configuration; a service may ignore them. Requests honour `asr.timeout` and `asr.retries`.

## Response

```json
{"segments": [{"start": 0.0, "end": 2.4, "text": "...", "words": [{"word": "...", "start": 0.0, "end": 0.5}]}]}
```

Times are seconds and must lie inside the audio; starts are monotonic and `end > start`.
Failure: a non-2xx status, preferably with `{"error": "..."}`.

## Launching the service

If `asr.worker_command` (a list: program, then arguments) is non-empty and nothing listens on the
endpoint, Blitzline starts it and waits up to three minutes. Because Blitzline launches it, the endpoint
must then be `http` on `localhost`, `127.0.0.1` or `[::1]`. The environment variable
`BLITZLINE_PARENT_PID` holds Blitzline's process id; exit when it disappears. stderr goes to
`blitzline-worker.log` in the temp directory. With `worker_command` empty, start the service yourself
and any `http(s)` endpoint works. The old `asr.backend = "local-worker"` is a deprecated alias for `http`
with a launched worker.

## Wiring up common services

- whisper.cpp `server`: needs a small adapter (it takes multipart, not this JSON).
- faster-whisper / any local model: wrap it in a ~50-line HTTP handler returning the shape above.
- Hosted API: same adapter idea, forwarding the decoded audio; keep the key in `asr.api_key_env`.
