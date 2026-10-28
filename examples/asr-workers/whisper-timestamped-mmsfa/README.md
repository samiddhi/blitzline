# whisper-timestamped + MMS-FA worker

One example of an ASR worker; Blitzline does not depend on it. See
[docs/asr-worker-protocol.md](../../../docs/asr-worker-protocol.md) for what a worker must do.

Needs an NVIDIA GPU (CUDA), `whisper-timestamped` and `torchaudio`. Model (`large-v3-turbo`) and
device are hardcoded in the script; word alignment covers Latin scripts only.

```toml
[asr]
backend = "http"
endpoint = "http://127.0.0.1:8765/transcribe"
worker_command = ["/path/to/python", "/path/to/asr_worker.py"]
```
