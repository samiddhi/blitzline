"""Convert transcripts while preserving source timing and exact examples.

Scope statement: implement pure subtitle and plaintext transformations.
Included: ASR JSON normalization, SRT parsing/rendering, cue validation,
word-based cue splitting, canonical text spans, and text replacement.
Excluded: running ASR (integrations/asr.py), files (storage.py), LLM calls
(integrations/llm.py), and selecting vocabulary (vocabulary.py).
Start here: normalize_asr or parse_srt creates cues; plaintext creates the
text/span pair used by Blitzer. Time values are integer milliseconds.
"""

import re
import textwrap
from dataclasses import replace

from blitzline.records import Cue, Span, Word, integer, milliseconds, text

TIMESTAMP = r"(\d{2,}):(\d{2}):(\d{2}),(\d{3})"


def validate_cues(cues: list[Cue], duration_ms: int) -> list[Cue]:
    """Check cue identity, ordering, text, and source bounds; permit overlaps."""
    seen = set()
    previous = -1
    for cue in cues:
        text(cue.id, "cue ID")
        text(cue.text, f"{cue.id} text")
        integer(cue.start_ms, f"{cue.id} start")
        integer(cue.end_ms, f"{cue.id} end", 1)
        if cue.id in seen or cue.start_ms < previous:
            raise ValueError(f"Duplicate or out-of-order cue: {cue.id}")
        if cue.end_ms <= cue.start_ms or cue.end_ms > duration_ms:
            raise ValueError(
                f"Cue {cue.id} has invalid bounds for {duration_ms}ms media"
            )
        seen.add(cue.id)
        previous = cue.start_ms
        validate_words(cue)
    return cues


def validate_words(cue: Cue) -> None:
    """Reject word alignments outside their cue or in reversed source order."""
    previous = cue.start_ms
    for word in cue.words:
        if not cue.start_ms <= word.start_ms < word.end_ms <= cue.end_ms:
            raise ValueError(f"{cue.id} contains an invalid aligned word interval")
        if word.start_ms < previous:
            raise ValueError(f"{cue.id} word times are out of order")
        previous = word.start_ms


def aligned_words(segment: dict) -> tuple[Word, ...]:
    """Read complete WhisperX word timing; fall back when any word is unaligned."""
    rows = segment.get("words", [])
    if not isinstance(rows, list):
        raise ValueError("ASR words must be an array")
    if any(
        not isinstance(row, dict) or "start" not in row or "end" not in row
        for row in rows
    ):
        return ()
    return tuple(
        Word(
            text(row.get("word"), "ASR word"),
            milliseconds(row["start"], "word start"),
            milliseconds(row["end"], "word end"),
        )
        for row in rows
    )


def word_spans(source: str, words: tuple[Word, ...]) -> list[tuple[int, int]]:
    """Locate aligned word strings in order without rewriting the source text."""
    spans = []
    cursor = 0
    for word in words:
        value = word.text.strip()
        start = source.find(value, cursor)
        if start < 0:
            return []
        spans.append((start, start + len(value)))
        cursor = start + len(value)
    return spans


def split_segment(segment: dict, settings: dict) -> list[Cue]:
    """Split aligned ASR segments at pauses, punctuation, or readability bounds."""
    source = text(segment.get("text"), "ASR segment text").strip()
    start = milliseconds(segment.get("start"), "segment start")
    end = milliseconds(segment.get("end"), "segment end")
    if end <= start:
        raise ValueError("ASR segment end must be after its start")
    words = aligned_words(segment)
    positions = word_spans(source, words)
    if not words or len(positions) != len(words):
        return [Cue("", start, end, source)]
    groups = []
    first = 0
    for index, word in enumerate(words):
        elapsed = word.end_ms - words[first].start_ms
        chars = positions[index][1] - positions[first][0]
        tail = index + 1 == len(words)
        gap = (
            not tail and words[index + 1].start_ms - word.end_ms >= settings["pause_ms"]
        )
        boundary = re.search(r"[.!?][\"'’”)]*$", word.text) is not None
        if not (
            tail
            or gap
            or boundary
            or elapsed >= settings["max_cue_ms"]
            or chars >= settings["line_length"] * 2
        ):
            continue
        left = 0 if first == 0 else positions[first][0]
        right = len(source) if tail else positions[index + 1][0]
        groups.append(
            Cue(
                "",
                words[first].start_ms,
                word.end_ms,
                source[left:right].strip(),
                words[first : index + 1],
            )
        )
        first = index + 1
    return groups


def normalize_asr(data: dict, duration_ms: int, settings: dict) -> list[Cue]:
    """Normalize Whisper/WhisperX segment JSON and assign deterministic cue IDs."""
    if not isinstance(data, dict) or not isinstance(data.get("segments"), list):
        raise ValueError("ASR JSON must contain a segments array")
    cues = []
    for segment in data["segments"]:
        if not isinstance(segment, dict):
            raise ValueError("Each ASR segment must be an object")
        if not isinstance(segment.get("text"), str):
            raise ValueError("ASR segment text must be a string")
        if not segment["text"].strip():
            continue
        cues.extend(split_segment(segment, settings))
    numbered = [
        replace(cue, id=f"cue-{index:06d}") for index, cue in enumerate(cues, 1)
    ]
    offset = integer(data.get("blitzline_audio_offset_ms", 0), "audio offset")
    if offset:
        numbered = [
            replace(
                cue,
                start_ms=cue.start_ms + offset,
                end_ms=cue.end_ms + offset,
                words=tuple(
                    replace(
                        word,
                        start_ms=word.start_ms + offset,
                        end_ms=word.end_ms + offset,
                    )
                    for word in cue.words
                ),
            )
            for cue in numbered
        ]
    return validate_cues(numbered, duration_ms)


def timestamp_ms(groups) -> int:
    """Parse SRT components, rejecting invalid minute or second values."""
    hours, minutes, seconds, millis = map(int, groups)
    if minutes > 59 or seconds > 59:
        raise ValueError("SRT minutes and seconds must be below 60")
    return ((hours * 60 + minutes) * 60 + seconds) * 1000 + millis


def parse_srt(raw: str, duration_ms: int) -> list[Cue]:
    """Parse strict SRT blocks with multiline UTF-8 text and optional BOM."""
    raw = raw.lstrip("\ufeff").replace("\r\n", "\n").strip()
    if not raw:
        return []
    cues = []
    for block in re.split(r"\n\s*\n", raw):
        lines = block.splitlines()
        number = len(cues) + 1
        if len(lines) < 3 or not lines[0].isdigit():
            raise ValueError(f"Malformed SRT block {number}")
        match = re.fullmatch(TIMESTAMP + r"\s+-->\s+" + TIMESTAMP, lines[1])
        if not match:
            raise ValueError(f"Invalid SRT timestamp in block {number}")
        cues.append(
            Cue(
                f"cue-{number:06d}",
                timestamp_ms(match.groups()[:4]),
                timestamp_ms(match.groups()[4:]),
                "\n".join(lines[2:]),
            )
        )
    return validate_cues(cues, duration_ms)


def format_time(value: int) -> str:
    """Render integer milliseconds in SRT's hours:minutes:seconds,millis format."""
    seconds, millis = divmod(value, 1000)
    minutes, seconds = divmod(seconds, 60)
    hours, minutes = divmod(minutes, 60)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d},{millis:03d}"


def render_srt(cues: list[Cue], line_length: int = 42) -> str:
    """Render sequential SRT cues; wrapping changes display whitespace only."""
    blocks = []
    for index, cue in enumerate(cues, 1):
        lines = []
        for line in cue.text.splitlines():
            lines.extend(
                textwrap.wrap(
                    line,
                    width=line_length,
                    break_long_words=False,
                    break_on_hyphens=False,
                )
                or [""]
            )
        blocks.append(
            f"{index}\n{format_time(cue.start_ms)} --> {format_time(cue.end_ms)}\n"
            + "\n".join(lines)
        )
    return "\n\n".join(blocks) + ("\n" if blocks else "")


def plaintext(cues: list[Cue]) -> tuple[str, list[Span]]:
    """Join cues with spaces and map each normalized cue's character interval."""
    parts, spans = [], []
    cursor = 0
    for cue in cues:
        value = " ".join(cue.text.split())
        spans.append(Span(cue.id, cursor, cursor + len(value)))
        parts.append(value)
        cursor += len(value) + 1
    return " ".join(parts), spans


def replace_text(cues: list[Cue], replacements: dict[str, str]) -> list[Cue]:
    """Replace specified cue text, dropping stale word alignments after edits."""
    result = []
    for cue in cues:
        value = replacements.get(cue.id, cue.text)
        text(value, f"{cue.id} replacement")
        result.append(
            replace(cue, text=value, words=cue.words if value == cue.text else ())
        )
    return result


def readability_issues(cues: list[Cue], settings: dict) -> list[dict]:
    """Report long or fast cues without truncating text or inventing timings."""
    issues = []
    for cue in cues:
        duration = cue.end_ms - cue.start_ms
        rate = len(cue.text) * 1000 / duration
        if duration > settings["max_cue_ms"] or rate > settings["max_chars_per_second"]:
            issues.append(
                {
                    "cue_id": cue.id,
                    "duration_ms": duration,
                    "chars_per_second": round(rate, 1),
                }
            )
    return issues
