"""Offload normalized audio to deployed Modal functions or HTTP workers.

Scope statement: own remote ASR transport, chunk receipts, and timestamp rebasing.
Included: bounded WAV uploads, environment-based HTTP authentication, transient
HTTP retries, durable Modal call IDs, cached chunk results, and timed validation.
Excluded: local ASR commands, subtitle splitting policy, GPU provisioning, and CLI.
Start here: transcribe_remote consumes a mono 16kHz WAV and returns ASR segments.
"""

import base64
import hashlib
import io
import os
import time
import wave
from copy import deepcopy
from pathlib import Path

from blitzline.integrations.llm import RequestFailure, post_json
from blitzline.records import PipelineError
from blitzline.storage import digest, read_json, write_json
from blitzline.transcript import normalize_asr


def audio_chunks(path: Path, seconds: int):
    """Yield bounded mono PCM WAV chunks with their source offset and duration."""
    try:
        with wave.open(str(path), "rb") as audio:
            if (
                audio.getnchannels() != 1
                or audio.getframerate() != 16000
                or audio.getsampwidth() != 2
            ):
                raise PipelineError(
                    "Remote ASR requires mono 16kHz 16-bit PCM WAV; supply media_settings to transcribe"
                )
            offset = 0
            while frames := audio.readframes(seconds * 16000):
                count = len(frames) // 2
                buffer = io.BytesIO()
                with wave.open(buffer, "wb") as chunk:
                    chunk.setnchannels(1)
                    chunk.setsampwidth(2)
                    chunk.setframerate(16000)
                    chunk.writeframes(frames)
                yield buffer.getvalue(), offset / 16000, count / 16000
                offset += count
    except (wave.Error, EOFError):
        raise PipelineError("Remote ASR input must be a normalized PCM WAV") from None


def validate_result(data, duration: float, settings: dict) -> dict:
    """Require usable timed segments before saving a paid transcription result."""
    if not isinstance(data, dict) or data.get("error"):
        raise PipelineError("Remote ASR returned an error or non-object response")
    data = deepcopy(data)
    data.pop("blitzline_audio_offset_ms", None)
    if isinstance(data.get("segments"), list):
        for segment in data["segments"]:
            if not isinstance(segment, dict):
                continue
            words = segment.get("words", [])
            if isinstance(words, list) and any(
                not isinstance(word, dict) or "start" not in word or "end" not in word
                for word in words
            ):
                # Match the normalizer's whole-segment fallback for incomplete alignment.
                segment.pop("words", None)
    try:
        normalize_asr(data, round(duration * 1000), settings)
    except (ValueError, TypeError, KeyError) as error:
        raise PipelineError(f"Invalid remote ASR transcript: {error}") from None
    return data


def invoke_http(
    audio: bytes,
    language: str,
    settings: dict,
    key: str,
    transport=post_json,
    sleeper=time.sleep,
) -> dict:
    """Send the documented JSON worker contract with a stable idempotency key."""
    headers = {"Idempotency-Key": key}
    if settings["api_key_env"]:
        secret = os.environ.get(settings["api_key_env"])
        if not secret:
            raise PipelineError(
                f"Set {settings['api_key_env']} for remote ASR authentication"
            )
        headers["Authorization"] = "Bearer " + secret
    payload = {
        "audio": base64.b64encode(audio).decode("ascii"),
        "filename": "audio.wav",
        "language": language,
        "model": settings["model"],
        "compute_type": settings["remote_compute_type"],
        "options": settings["options"],
    }
    for attempt in range(settings["retries"] + 1):
        try:
            return transport(
                settings["endpoint"], payload, headers, settings["timeout"]
            )
        except RequestFailure as error:
            if not error.retryable or attempt == settings["retries"]:
                raise
            sleeper(min(2**attempt, 30))
    raise AssertionError("Unreachable retry state")


def invoke_modal(
    audio: bytes, language: str, settings: dict, receipt: Path | None
) -> dict:
    """Persist a spawned call ID and recover the same job after client interruption."""
    try:
        import modal
    except ImportError:
        raise PipelineError(
            "Modal ASR requires: pip install 'blitzline[modal]' and modal setup"
        ) from None
    try:
        previous = read_json(receipt) if receipt and receipt.is_file() else {}
        if previous.get("call_id"):
            call = modal.FunctionCall.from_id(previous["call_id"])
        else:
            function = modal.Function.from_name(
                settings["modal_app"],
                settings["modal_function"],
                environment_name=settings["modal_environment"] or None,
            )
            call = function.spawn(
                audio=audio,
                language=language,
                model=settings["model"],
                compute_type=settings["remote_compute_type"],
                options=settings["options"],
            )
            if receipt:
                write_json(receipt, {"call_id": call.object_id})
        return call.get(timeout=settings["timeout"])
    except Exception as error:
        # Remote exception strings can contain request bodies or credentials.
        raise PipelineError(
            f"Modal ASR failed ({type(error).__name__}); check Modal authentication, deployment, and function logs. Resume reuses the saved call ID."
        ) from None


def transcribe_remote(
    path: Path,
    language: str,
    settings: dict,
    cache_dir=None,
    *,
    transport=post_json,
    sleeper=time.sleep,
) -> dict:
    """Reuse completed chunks, invoke the selected worker, and rebase all timestamps."""
    segments = []
    for audio, offset, duration in audio_chunks(path, settings["chunk_seconds"]):
        identity = {
            "contract": 1,
            "audio": hashlib.sha256(audio).hexdigest(),
            "language": language,
            **{
                k: settings[k]
                for k in (
                    "backend",
                    "endpoint",
                    "modal_app",
                    "modal_function",
                    "modal_environment",
                    "model",
                    "remote_compute_type",
                    "options",
                )
            },
        }
        key = digest(identity)
        result_path = Path(cache_dir) / (key + ".json") if cache_dir else None
        receipt = Path(cache_dir) / (key + ".modal.json") if cache_dir else None
        if result_path and result_path.is_file():
            cached = read_json(result_path)
            data = validate_result(cached["result"], duration, settings)
            if cached.get("checksum") != digest(data):
                raise PipelineError(
                    "Remote ASR cache checksum mismatch; remove the affected chunk cache to retry"
                )
        else:
            if settings["backend"] == "modal":
                data = invoke_modal(audio, language, settings, receipt)
            else:
                data = invoke_http(audio, language, settings, key, transport, sleeper)
            data = validate_result(data, duration, settings)
            if result_path:
                write_json(result_path, {"result": data, "checksum": digest(data)})
        for segment in deepcopy(data["segments"]):
            if not segment["text"].strip():
                continue
            for bound in ("start", "end"):
                segment[bound] += offset
            for word in segment.get("words", []):
                for bound in ("start", "end"):
                    if bound in word:
                        word[bound] += offset
            segments.append(segment)
    return {
        "segments": segments,
        "language": language,
        "blitzline_asr_backend": settings["backend"],
    }
