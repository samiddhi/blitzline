"""Probe recordings and produce accurately timed example clips.

Scope statement: own media commands and pure clip plans.
Included: checked process execution, ffprobe normalization, padded interval
validation, deterministic filenames, verified cache reuse, and ffmpeg cuts.
Excluded: subtitle text (transcript.py), vocabulary selection (vocabulary.py),
LLM decisions (integrations/llm.py), and Anki field rendering (exports/common.py).
Start here: probe checks input; clip_plan is pure; extract_clip performs I/O.
"""

import json
import os
import shutil
import subprocess
from pathlib import Path

from blitzline.records import PipelineError, milliseconds
from blitzline.storage import digest, fingerprint, read_json, write_json


def run_process(arguments: list[str], timeout: int, runner=subprocess.run):
    """Run an argument list without a shell and return checked UTF-8 stdout."""
    try:
        result = runner(
            arguments, capture_output=True, text=True, encoding="utf-8", timeout=timeout
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise PipelineError(f"Cannot run {arguments[0]}: {error}") from error
    if result.returncode:
        raise PipelineError(
            f"{arguments[0]} failed ({result.returncode}): {result.stderr[-4000:]}"
        )
    return result.stdout


def tool_identity(executable: str) -> dict:
    """Fingerprint a resolved tool executable without launching or importing it."""
    resolved = shutil.which(executable) or executable
    return {"executable": resolved, "checksum": fingerprint(resolved)}


def probe(path: Path, settings: dict, runner=subprocess.run) -> dict:
    """Return recording duration and streams, requiring a readable audio stream."""
    if not path.is_file():
        raise ValueError(f"Media file does not exist: {path}")
    raw = run_process(
        [
            settings["ffprobe"],
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            str(path),
        ],
        settings["timeout"],
        runner,
    )
    try:
        data = json.loads(raw)
        duration = milliseconds(float(data["format"]["duration"]), "media duration")
        audio = next(s for s in data["streams"] if s.get("codec_type") == "audio")
    except (ValueError, KeyError, StopIteration, TypeError) as error:
        raise PipelineError(
            f"ffprobe found no usable audio/duration in {path}"
        ) from error
    video = next((s for s in data["streams"] if s.get("codec_type") == "video"), None)
    if duration <= 0:
        raise ValueError("Media duration must be positive")
    origin = float(data["format"].get("start_time", 0))
    audio_origin = float(audio.get("start_time", origin))
    offset = milliseconds(max(0, audio_origin - origin), "audio stream offset")
    return {
        "duration_ms": duration,
        "audio_index": audio["index"],
        "video_index": video["index"] if video else None,
        "audio_offset_ms": offset,
    }


def clip_plan(
    context, source_hash: str, info: dict, settings: dict, encoder_identity=None
) -> dict:
    """Plan a bounded clip without accessing files; reject unaligned/long examples."""
    if context.error or not context.cue_ids:
        raise ValueError(f"Context {context.id} lacks reliable timing")
    start = max(0, context.start_ms - settings["padding_ms"])
    end = min(info["duration_ms"], context.end_ms + settings["padding_ms"])
    if end <= start or end - start > settings["max_clip_ms"]:
        raise ValueError(
            f"Context {context.id} needs review: invalid or overly long clip"
        )
    if settings["kind"] == "video" and info["video_index"] is None:
        raise ValueError(
            "Video clips require a source video stream; select audio instead"
        )
    identity = {
        "source": source_hash,
        "start_ms": start,
        "end_ms": end,
        "kind": settings["kind"],
        "audio_codec": settings["audio_codec"],
        "video_codec": settings["video_codec"],
        "encoder": encoder_identity,
    }
    extension = "mp4" if settings["kind"] == "video" else "mp3"
    return {**identity, "filename": f"blitzline-{digest(identity)[:24]}.{extension}"}


def extract_clip(
    source: Path,
    directory: Path,
    plan: dict,
    info: dict,
    settings: dict,
    runner=subprocess.run,
) -> Path:
    """Encode a clip atomically and cache only verified stream/duration results."""
    path = directory / plan["filename"]
    receipt = directory / (plan["filename"] + ".json")
    if path.is_file() and receipt.is_file():
        saved = read_json(receipt)
        if saved.get("plan") == plan and saved.get("checksum") == fingerprint(path):
            return path
    directory.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(".encoding-" + path.name)
    seconds = (plan["end_ms"] - plan["start_ms"]) / 1000
    args = [
        settings["ffmpeg"],
        "-nostdin",
        "-v",
        "error",
        "-y",
        "-i",
        str(source),
        "-ss",
        f"{plan['start_ms'] / 1000:.3f}",
        "-t",
        f"{seconds:.3f}",
        "-map",
        f"0:{info['audio_index']}",
    ]
    if settings["kind"] == "video":
        args += [
            "-map",
            f"0:{info['video_index']}",
            "-c:v",
            settings["video_codec"],
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
        ]
    else:
        args += ["-vn", "-c:a", settings["audio_codec"], "-q:a", "2"]
    try:
        run_process([*args, str(temporary)], settings["timeout"], runner)
        verified = probe(temporary, settings, runner)
        if abs(verified["duration_ms"] - round(seconds * 1000)) > 500:
            raise PipelineError(f"Encoded clip duration is incorrect: {path.name}")
        if settings["kind"] == "video" and verified["video_index"] is None:
            raise PipelineError("Encoded video clip contains no video")
        os.replace(temporary, path)
        write_json(receipt, {"plan": plan, "checksum": fingerprint(path)})
    finally:
        temporary.unlink(missing_ok=True)
    return path
