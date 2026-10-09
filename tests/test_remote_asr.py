"""Verify remote transcription contracts without GPU jobs or paid network calls.

Scope statement: exercise remote ASR chunks, authentication, timing, and recovery.
Included: HTTP request data, retries, cache reuse, Modal job recovery, real local
WAV preparation, pipeline resume, re-listening, and response/config validation.
Excluded: speech recognition accuracy, deployed infrastructure, and real secrets.
Start here: test_http_chunks verifies bytes and absolute timestamps independently.
"""

import base64
import io
import struct
import sys
import types
import wave
from copy import deepcopy

import pytest

from blitzline.config import validate_config
from blitzline.core import resume_pipeline, run_pipeline
from blitzline.integrations.asr import relisten, transcribe
from blitzline.integrations.llm import RequestFailure
from blitzline.integrations.remote_asr import transcribe_remote
from blitzline.media import probe
from blitzline.records import Cue, PipelineError
from blitzline.storage import read_json, write_json


def wav_fixture(tmp_path, seconds=2.5):
    """Create normalized PCM with distinguishable bytes for each one-second chunk."""
    path = tmp_path / "audio.wav"
    with wave.open(str(path), "wb") as audio:
        audio.setnchannels(1)
        audio.setsampwidth(2)
        audio.setframerate(16000)
        audio.writeframes(
            b"".join(
                struct.pack("<h", index // 16000)
                for index in range(round(seconds * 16000))
            )
        )
    return path


def transcript():
    """Return one segment with aligned words entirely within the shortest test chunk."""
    return {
        "segments": [
            {
                "start": 0.1,
                "end": 0.4,
                "text": "Test.",
                "words": [{"word": "Test.", "start": 0.1, "end": 0.4}],
            }
        ]
    }


def http_settings(settings):
    """Select the HTTP backend with a deliberately small chunk size for tests."""
    settings["asr"].update(
        backend="http",
        endpoint="https://asr.example.test/transcribe",
        api_key_env="ASR_TEST_KEY",
        chunk_seconds=1,
    )
    return settings["asr"]


def test_http_chunks_and_cache(tmp_path, settings, monkeypatch):
    """Send bounded WAVs with credentials only in headers and rebase word timestamps."""
    path = wav_fixture(tmp_path)
    options = http_settings(settings)
    monkeypatch.setenv("ASR_TEST_KEY", "private-test-key")
    calls = []

    def transport(url, payload, headers, timeout):
        """Inspect real serialized audio and return segment-relative word timings."""
        calls.append((url, payload, headers, timeout))
        with wave.open(io.BytesIO(base64.b64decode(payload["audio"]))) as audio:
            assert audio.getframerate() == 16000 and audio.getnchannels() == 1
            assert audio.getnframes() <= 16000
        return transcript()

    cache = tmp_path / "cache"
    result = transcribe_remote(path, "sl", options, cache, transport=transport)
    assert [s["start"] for s in result["segments"]] == [0.1, 1.1, 2.1]
    assert result["segments"][2]["words"][0]["end"] == 2.4
    assert len(calls) == 3 and len({c[2]["Idempotency-Key"] for c in calls}) == 3
    assert calls[0][1]["language"] == "sl"
    assert calls[0][2]["Authorization"] == "Bearer private-test-key"
    monkeypatch.delenv("ASR_TEST_KEY")
    assert transcribe_remote(path, "sl", options, cache, transport=transport) == result
    assert len(calls) == 3
    assert all("private-test-key" not in p.read_text() for p in cache.glob("*.json"))
    options["options"] = {"beam_size": 3}
    with pytest.raises(PipelineError, match="Set ASR_TEST_KEY"):
        transcribe_remote(path, "sl", options, cache, transport=transport)


def test_http_partial_resume_and_retry(tmp_path, settings):
    """Reuse completed chunks after a failure and bound transient HTTP retries."""
    path = wav_fixture(tmp_path)
    options = http_settings(settings)
    options.update(api_key_env="", retries=0)
    calls = []

    def transport(url, payload, headers, timeout):
        """Fail the second chunk once, after the first chunk was durably cached."""
        calls.append(headers["Idempotency-Key"])
        if len(calls) == 2:
            raise RequestFailure("HTTP 503", True)
        return transcript()

    cache = tmp_path / "cache"
    with pytest.raises(RequestFailure):
        transcribe_remote(path, "en", options, cache, transport=transport)
    result = transcribe_remote(path, "en", options, cache, transport=transport)
    assert len(result["segments"]) == 3
    assert len(calls) == 4 and calls[1] == calls[2] and calls.count(calls[0]) == 1
    options["retries"] = 2
    waits = []
    attempts = []

    def rate_limited(*args):
        """Simulate an exhausted retry budget without actual waiting."""
        attempts.append(1)
        raise RequestFailure("HTTP 429", True)

    with pytest.raises(RequestFailure):
        transcribe_remote(
            path, "en", options, transport=rate_limited, sleeper=waits.append
        )
    assert len(attempts) == 3 and waits == [1, 2]


@pytest.mark.parametrize(
    "response",
    [
        {"text": "No timing"},
        {"segments": [{"text": "Bad", "start": -1, "end": 0.4}]},
        {"segments": [{"text": "Beyond chunk", "start": 0.1, "end": 99}]},
        {"error": "private response body"},
    ],
)
def test_bad_remote_results_not_cached(tmp_path, settings, response):
    """Reject missing/invalid timing before persisting a successful chunk result."""
    options = http_settings(settings)
    options["api_key_env"] = ""
    cache = tmp_path / "cache"
    with pytest.raises(PipelineError):
        transcribe_remote(
            wav_fixture(tmp_path),
            "en",
            options,
            cache,
            transport=lambda *args: response,
        )
    assert not list(cache.glob("*.json"))


def test_modal_resume_existing_call(tmp_path, settings, monkeypatch):
    """Recover a timed-out Modal call using its saved ID instead of spawning twice."""
    path = wav_fixture(tmp_path, 0.5)
    options = settings["asr"]
    options.update(backend="modal", modal_environment="production")
    spawned = []
    recovered = []

    class Call:
        """Represent a durable job whose first client wait times out."""

        object_id = "fc-test-job"

        def get(self, timeout):
            """Finish only after the client recovers this call by its ID."""
            if not recovered:
                raise TimeoutError("do not expose request or credentials")
            return transcript()

    class Function:
        """Emulate the Modal SDK's deployed function lookup and spawn contract."""

        @staticmethod
        def from_name(app, function, *, environment_name):
            """Verify the configured deployment and return a callable function handle."""
            assert (app, function, environment_name) == (
                "blitzline-asr",
                "transcribe",
                "production",
            )
            return Function()

        def spawn(self, **kwargs):
            """Capture the normalized WAV and decoding settings exactly once."""
            spawned.append(kwargs)
            return Call()

    class FunctionCall:
        """Emulate restoring a durable handle without invoking a new function."""

        @staticmethod
        def from_id(identity):
            """Record recovery and allow the existing job to finish."""
            recovered.append(identity)
            return Call()

    monkeypatch.setitem(
        sys.modules,
        "modal",
        types.SimpleNamespace(Function=Function, FunctionCall=FunctionCall),
    )
    cache = tmp_path / "cache"
    with pytest.raises(PipelineError, match="Modal ASR failed") as failure:
        transcribe_remote(path, "en", options, cache)
    assert "credentials" not in str(failure.value)
    assert read_json(next(cache.glob("*.modal.json")))["call_id"] == "fc-test-job"
    result = transcribe_remote(path, "en", options, cache)
    assert result["segments"] and recovered == ["fc-test-job"] and len(spawned) == 1
    assert spawned[0]["audio"].startswith(b"RIFF")
    assert spawned[0]["compute_type"] == "float16"


def test_remote_pipeline_normalization_and_relisten(
    tmp_path, recording, settings, monkeypatch
):
    """Use real FFmpeg with remote ASR and resume without requiring local WhisperX."""
    options = http_settings(settings)
    options.update(api_key_env="", chunk_seconds=600, executable="no-local-whisperx")
    calls = []

    def worker(audio, language, settings, key, transport, sleeper):
        """Validate actual extracted/cut audio while replacing the remote service."""
        with wave.open(io.BytesIO(audio)) as wav:
            assert wav.getframerate() == 16000 and wav.getnchannels() == 1
        calls.append(language)
        return transcript()

    monkeypatch.setattr("blitzline.integrations.remote_asr.invoke_http", worker)
    directory = run_pipeline(recording, "eng", settings, stop_after="transcript")
    raw = read_json(directory / "artifacts/raw-asr.json")
    assert raw["blitzline_asr_backend"] == "http"
    resume_pipeline(directory, stop_after="transcript")
    assert calls == ["en"]
    info = probe(recording, settings["media"])
    info["audio_offset_ms"] = 50
    data = transcribe(
        recording,
        "eng",
        options,
        media_settings=settings["media"],
        info=info,
        cache_dir=directory / ".cache/asr",
    )
    assert data["blitzline_audio_offset_ms"] == 50
    assert (
        relisten(
            recording,
            Cue("cue", 100, 600, "Question"),
            "eng",
            options,
            settings["media"],
            cache_dir=directory / ".cache/asr",
        )
        == "Test."
    )


def test_remote_cache_integrity(tmp_path, settings):
    """Detect edited cached transcription instead of silently accepting changed text."""
    options = http_settings(settings)
    options["api_key_env"] = ""
    path = wav_fixture(tmp_path, 0.5)
    cache = tmp_path / "cache"
    transcribe_remote(path, "en", options, cache, transport=lambda *args: transcript())
    saved = next(cache.glob("*.json"))
    data = read_json(saved)
    data["result"]["segments"][0]["text"] = "Edited."
    write_json(saved, data)
    with pytest.raises(PipelineError, match="checksum"):
        transcribe_remote(path, "en", options, cache)


@pytest.mark.parametrize(
    "change",
    [
        {"backend": "unknown"},
        {"backend": "http", "endpoint": "https://secret:password@host.test"},
        {"api_key_env": "not an env name"},
        {"chunk_seconds": 0},
        {"backend": "modal", "modal_app": ""},
    ],
)
def test_remote_config_validation(settings, change):
    """Validate backend choices, endpoint credentials, chunk sizes, and deployments."""
    settings = deepcopy(settings)
    settings["asr"].update(change)
    with pytest.raises(ValueError):
        validate_config(settings)


def test_partial_word_alignment_falls_back(tmp_path, settings):
    """Drop incomplete alignments before rebasing while retaining segment timing."""
    options = http_settings(settings)
    options["api_key_env"] = ""
    result = transcribe_remote(
        wav_fixture(tmp_path),
        "en",
        options,
        transport=lambda *args: {
            "segments": [
                {
                    "start": 0.1,
                    "end": 0.4,
                    "text": "Speech",
                    "words": [{"word": "Speech", "start": None}],
                }
            ]
        },
    )
    assert len(result["segments"]) == 3
    assert result["segments"][2]["start"] == 2.1
    assert all("words" not in s for s in result["segments"])
