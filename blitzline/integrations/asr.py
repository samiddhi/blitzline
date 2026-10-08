"""Launch WhisperX without importing its heavyweight runtime.

Scope statement: own transcription command execution and raw JSON discovery.
Included: pack-to-ASR language mapping, configured model/device execution,
and targeted second transcription for questionable cues.
Excluded: ASR normalization and cue splitting (transcript.py), LLM text review
(llm.py), final media clips (media.py), and command-line presentation (cli.py).
Start here: transcribe returns raw segment JSON; relisten retranscribes one cue.
"""

import subprocess
import tempfile
from pathlib import Path

from blitzline.media import run_process
from blitzline.records import PipelineError
from blitzline.storage import read_json

LANGUAGES = {
    "slv": "sl",
    "pol": "pl",
    "eng": "en",
    "deu": "de",
    "fra": "fr",
    "spa": "es",
    "ita": "it",
    "por": "pt",
    "nld": "nl",
    "ces": "cs",
    "hrv": "hr",
    "srp": "sr",
    "rus": "ru",
    "ukr": "uk",
    "jpn": "ja",
    "zho": "zh",
    "kor": "ko",
    "ara": "ar",
    "hin": "hi",
    "swe": "sv",
    "dan": "da",
    "nor": "no",
    "fin": "fi",
    "hun": "hu",
    "tur": "tr",
}


def asr_language(language: str, settings: dict) -> str:
    """Select an explicit ASR language or map a supported Blitzer pack code."""
    selected = settings["language"] or LANGUAGES.get(language)
    if not selected:
        raise ValueError(
            f"No ASR code mapped for {language}; set asr.language or import a transcript"
        )
    return selected


def transcribe(
    source: Path,
    language: str,
    settings: dict,
    runner=subprocess.run,
    *,
    media_settings=None,
    info=None,
) -> dict:
    """Run configured WhisperX for raw JSON in a temporary working directory."""
    with tempfile.TemporaryDirectory(prefix="blitzline-asr-") as directory:
        selected_source = source
        if media_settings is not None:
            selected_source = Path(directory) / "asr-input.wav"
            command = [
                media_settings["ffmpeg"],
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-i",
                str(source),
                "-map",
                f"0:{info['audio_index']}",
                "-vn",
                "-ar",
                "16000",
                "-ac",
                "1",
                str(selected_source),
            ]
            run_process(command, settings["timeout"], runner)
        args = [
            settings["executable"],
            str(selected_source),
            "--model",
            settings["model"],
            "--device",
            settings["device"],
            "--compute_type",
            settings["compute_type"],
            "--language",
            asr_language(language, settings),
            "--output_format",
            "json",
            "--output_dir",
            directory,
        ]
        run_process(args, settings["timeout"], runner)
        path = Path(directory) / (selected_source.stem + ".json")
        if not path.is_file():
            raise PipelineError(f"ASR produced no expected JSON: {path.name}")
        data = read_json(path)
        if media_settings is not None:
            data["blitzline_audio_offset_ms"] = info.get("audio_offset_ms", 0)
        return data


def relisten(
    source: Path,
    cue,
    language: str,
    settings: dict,
    media_settings: dict,
    runner=subprocess.run,
) -> str:
    """Run a second ASR pass on one exact interval; return text without new timing."""
    with tempfile.TemporaryDirectory(prefix="blitzline-relisten-") as directory:
        audio = Path(directory) / "question.wav"
        args = [
            media_settings["ffmpeg"],
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-i",
            str(source),
            "-map",
            "0:a:0",
            "-ss",
            str(cue.start_ms / 1000),
            "-t",
            str((cue.end_ms - cue.start_ms) / 1000),
            "-vn",
            "-ar",
            "16000",
            "-ac",
            "1",
            str(audio),
        ]
        run_process(args, media_settings["timeout"], runner)
        data = transcribe(audio, language, settings, runner)
        value = " ".join(
            segment["text"].strip() for segment in data.get("segments", [])
        ).strip()
        if not value:
            raise PipelineError(f"Second ASR found no text for {cue.id}")
        return value
