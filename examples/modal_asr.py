"""Deploy a GPU transcription worker with `modal deploy examples/modal_asr.py`.

Scope statement: provide a deployable implementation of Blitzline's ASR contract.
Included: CUDA/cuDNN runtime, persistent model downloads, word timestamps, and
configurable decoding options using faster-whisper on a Modal GPU.
Excluded: subtitle normalization, local media extraction, note generation, and CLI.
Start here: deploy this app, then select asr.backend = "modal" in Blitzline TOML.
"""

import io

import modal

app = modal.App("blitzline-asr")
image = modal.Image.from_registry(
    "nvidia/cuda:12.3.2-cudnn9-runtime-ubuntu22.04", add_python="3.11"
).pip_install("faster-whisper>=1.1,<2")
models = modal.Volume.from_name("blitzline-asr-models", create_if_missing=True)


@app.function(image=image, gpu="L4", timeout=7200, volumes={"/models": models})
def transcribe(
    audio: bytes, language: str, model: str, compute_type: str, options: dict
) -> dict:
    """Return timed segments and words relative to the supplied mono 16kHz WAV."""
    from faster_whisper import WhisperModel

    recognizer = WhisperModel(
        model, device="cuda", compute_type=compute_type, download_root="/models"
    )
    decoding = {"word_timestamps": True, "vad_filter": True, **options}
    segments, info = recognizer.transcribe(
        io.BytesIO(audio), language=language, **decoding
    )
    result = []
    for segment in segments:
        row = {"start": segment.start, "end": segment.end, "text": segment.text}
        if segment.words:
            row["words"] = [
                {"word": w.word, "start": w.start, "end": w.end, "score": w.probability}
                for w in segment.words
                if w.end > w.start
            ]
        result.append(row)
    return {"segments": result, "language": info.language}
