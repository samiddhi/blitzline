"""Check subtitle and text transformations without external services.

Scope statement: exercise exact text, Unicode offsets, and timing invariants.
Included: SRT round trips, ASR word boundaries, blank/silent transcripts,
malformed timing, cue overlaps, and stale alignment after corrections.
Excluded: launching WhisperX or reading user recordings (test_pipeline.py).
Start here: test_srt_roundtrip checks our local SRT writer and parser.
"""

import pytest

from blitzline.records import Cue, Word
from blitzline.transcript import (
    normalize_asr,
    parse_srt,
    plaintext,
    readability_issues,
    render_srt,
    replace_text,
    validate_cues,
)


def test_srt_roundtrip(cues):
    """Preserve cue text/times with BOM, multiline cues, and hour boundaries."""
    source = [*cues, Cue("cue-000005", 3600999, 3601999, "Zażółć\nĀnanda <&>.")]
    result = parse_srt("\ufeff" + render_srt(source), 3602000)
    assert result == source
    assert "01:00:00,999" in render_srt(source)


@pytest.mark.parametrize(
    "raw",
    [
        "1\n00:99:00,000 --> 00:00:01,000\nword",
        "1\n00:00:02,000 --> 00:00:01,000\nword",
        "broken",
    ],
)
def test_bad_srt(raw):
    """Reject malformed and reversed SRT timing with useful errors."""
    with pytest.raises(ValueError):
        parse_srt(raw, 4000)


def test_plaintext_spans(cues):
    """Keep punctuation and record exact cue offsets through whitespace normalization."""
    source, spans = plaintext(cues)
    assert source == "Mačka spi. Mačka spi. Pes teče."
    assert source[spans[2].start : spans[2].end] == "Pes"


def test_asr_word_splitting(settings):
    """Split complete aligned words at punctuation while retaining exact original text."""
    raw = {
        "segments": [
            {
                "start": 0.1,
                "end": 2.0,
                "text": "Živjo!  Kako si?",
                "words": [
                    {"word": "Živjo!", "start": 0.1, "end": 0.5},
                    {"word": "Kako", "start": 1.0, "end": 1.5},
                    {"word": "si?", "start": 1.6, "end": 2.0},
                ],
            }
        ]
    }
    cues = normalize_asr(raw, 2500, settings["asr"])
    assert [c.text for c in cues] == ["Živjo!", "Kako si?"]
    assert cues[0].end_ms == 500
    assert cues[1].start_ms == 1000


def test_missing_word_timing(settings):
    """Fall back to segment timing when WhisperX leaves a word unaligned."""
    raw = {
        "segments": [
            {
                "start": 0,
                "end": 1,
                "text": "Word 42.",
                "words": [{"word": "Word", "start": 0, "end": 0.5}, {"word": "42."}],
            }
        ]
    }
    result = normalize_asr(raw, 1000, settings["asr"])
    assert result[0].words == ()
    assert result[0].text == "Word 42."


def test_silence_and_invalid_times(settings):
    """Accept empty transcripts and reject nonfinite ASR timestamps."""
    assert normalize_asr({"segments": []}, 1000, settings["asr"]) == []
    assert parse_srt("", 1000) == []
    with pytest.raises(ValueError):
        normalize_asr(
            {"segments": [{"start": float("nan"), "end": 1, "text": "Hi"}]},
            2000,
            settings["asr"],
        )


def test_correction_drops_alignment():
    """Discard stale word offsets only for cues whose text actually changed."""
    cue = Cue("cue-1", 0, 1000, "Hi.", (Word("Hi.", 0, 1000),))
    assert replace_text([cue], {"cue-1": "Bye."})[0].words == ()
    assert replace_text([cue], {"cue-1": "Hi."})[0].words == cue.words


def test_valid_overlaps():
    """Allow overlapping speech while rejecting out-of-order cues and word timing."""
    assert validate_cues([Cue("a", 0, 1000, "A"), Cue("b", 500, 1500, "B")], 2000)
    with pytest.raises(ValueError):
        validate_cues([Cue("a", 0, 1000, "A", (Word("A", 900, 1100),))], 2000)


def test_readability_does_not_truncate(settings):
    """Report fast reading while preserving the full cue and its source interval."""
    cue = Cue("cue-1", 0, 100, "A full sentence that must remain unchanged.")
    assert readability_issues([cue], settings["asr"])[0]["cue_id"] == cue.id
    assert cue.text.endswith("unchanged.")


def test_audio_stream_offset(settings):
    """Restore the source origin for ASR run on extracted audio that starts late."""
    raw = {
        "blitzline_audio_offset_ms": 500,
        "segments": [{"start": 0, "end": 1, "text": "Delayed speech."}],
    }
    cue = normalize_asr(raw, 2000, settings["asr"])[0]
    assert (cue.start_ms, cue.end_ms) == (500, 1500)
