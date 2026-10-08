"""Check external tool adapters and proofreading capability boundaries.

Scope statement: verify command contracts with temporary outputs and fake runners.
Included: WhisperX JSON discovery, language codes, command errors, clip limits,
Blitzer automation flags, proofreading uncertainty, and second-ASR evidence.
Excluded: paid APIs, downloaded speech models, or personal recordings.
Start here: test_asr_command checks the command built for a separate ASR environment.
"""

import subprocess
from pathlib import Path

import pytest
from conftest import configure_llm

from blitzline.core import prepare_proofread
from blitzline.integrations.asr import asr_language, relisten, transcribe
from blitzline.integrations.blitzer import arguments
from blitzline.media import clip_plan, probe, run_process
from blitzline.records import Context, Cue, PipelineError
from blitzline.storage import RunStore, write_json


def test_asr_command(settings, tmp_path):
    """Run the configured model/language and discover the expected raw JSON file."""
    calls = []

    def runner(args, **kwargs):
        """Write the JSON that WhisperX's selected output format would produce."""
        calls.append(args)
        output = Path(args[args.index("--output_dir") + 1])
        write_json(output / "source.json", {"segments": []})
        return subprocess.CompletedProcess(args, 0, "", "")

    result = transcribe(tmp_path / "source.mp4", "slv", settings["asr"], runner)
    assert result == {"segments": []}
    assert calls[0][calls[0].index("--language") + 1] == "sl"
    assert calls[0][calls[0].index("--output_format") + 1] == "json"
    with pytest.raises(ValueError, match="asr.language"):
        asr_language("pli", settings["asr"])


def test_blitzer_invariants(settings, tmp_path):
    """Always request required extraction options and explicitly disable user-data writes."""
    args = arguments("slv", tmp_path / "text.txt", settings["blitzer"])
    for flag in (
        "--freq",
        "--lemmatize",
        "--context",
        "--no-prompt",
        "--no-src",
        "--no-save-context",
        "--no-update-known",
        "--no-config",
    ):
        assert flag in args
    assert args[args.index("--format") + 1] == "json"
    assert args[args.index("--bold") + 1] == "html"


def test_process_failure():
    """Convert nonzero process status and timeouts into actionable domain errors."""

    def failed(args, **kwargs):
        """Return a failing subprocess result."""
        return subprocess.CompletedProcess(args, 9, "", "diagnostic")

    with pytest.raises(PipelineError, match="diagnostic"):
        run_process(["tool"], 1, failed)

    def timed_out(args, **kwargs):
        """Simulate an external process exceeding its configured timeout."""
        raise subprocess.TimeoutExpired(args, 1)

    with pytest.raises(PipelineError, match="Cannot run"):
        run_process(["tool"], 1, timed_out)


def test_clip_bounds(settings):
    """Clamp padding, reject missing video, and reject clips beyond configured limits."""
    context = Context("context", "Word.", 0, 4, ("cue",), 100, 950)
    info = {"duration_ms": 1000, "video_index": None, "audio_index": 0}
    plan = clip_plan(context, "hash", info, settings["media"])
    assert (plan["start_ms"], plan["end_ms"]) == (0, 1000)
    settings["media"]["kind"] = "video"
    with pytest.raises(ValueError, match="Video"):
        clip_plan(context, "hash", info, settings["media"])
    settings["media"].update(kind="audio", max_clip_ms=500)
    with pytest.raises(ValueError, match="long"):
        clip_plan(context, "hash", info, settings["media"])


def test_uncertain_proofreading_needs_evidence(settings, tmp_path, cues, monkeypatch):
    """Keep uncertain cues unresolved until a configured second-ASR pass resolves them."""
    configure_llm(settings)
    settings["run"]["proofread"] = True
    store = RunStore(tmp_path / "run", {})
    calls = []

    def requester(profile, instruction, data, schema, cache):
        """Request re-listening first, then accept the supplied second-ASR evidence."""
        calls.append(data)
        has_evidence = bool(data["second_asr"])
        return {
            "cues": [
                {
                    "cue_id": row["cue_id"],
                    "text": row["text"],
                    "uncertain": not has_evidence,
                    "reason": "" if has_evidence else "Ambiguous audio",
                }
                for row in data["requested"]
            ]
        }

    result, paths = prepare_proofread(
        store, tmp_path / "recording", "slv", cues, settings, requester
    )
    assert result["errors"]
    settings["asr"]["relisten"] = True

    def relisten(*args, **kwargs):
        """Supply independently transcribed evidence for the questionable cue."""
        return "Independent ASR text"

    monkeypatch.setattr("blitzline.core.asr.relisten", relisten)
    result, paths = prepare_proofread(
        store, tmp_path / "recording", "slv", cues, settings, requester
    )
    assert not result["errors"]
    assert calls[-1]["second_asr"]


def test_normalized_audio_and_relisten_commands(settings, recording):
    """Actually convert/cut audio while substituting only the unavailable ASR model."""

    def runner(args, **kwargs):
        """Run real FFmpeg or emit a raw ASR response for the requested WAV."""
        if args[0] != settings["asr"]["executable"]:
            return subprocess.run(args, **kwargs)
        source = Path(args[1])
        assert source.is_file() and source.suffix == ".wav"
        directory = Path(args[args.index("--output_dir") + 1])
        write_json(
            directory / (source.stem + ".json"),
            {"segments": [{"start": 0, "end": 0.2, "text": "Test speech."}]},
        )
        return subprocess.CompletedProcess(args, 0, "", "")

    info = probe(recording, settings["media"])
    result = transcribe(
        recording,
        "eng",
        settings["asr"],
        runner,
        media_settings=settings["media"],
        info=info,
    )
    assert result["blitzline_audio_offset_ms"] == info["audio_offset_ms"]
    assert (
        relisten(
            recording,
            Cue("cue", 200, 700, "Question"),
            "eng",
            settings["asr"],
            settings["media"],
            runner,
        )
        == "Test speech."
    )
