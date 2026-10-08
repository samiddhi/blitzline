"""Define the values passed between pipeline stages.

Scope statement: describe stable data and validate primitive values.
Included: immutable cue, word, span, context, candidate, and card records;
JSON reconstruction; schema versions; domain error types.
Excluded: file access (storage.py), text algorithms (transcript.py),
provider calls (integrations/), and terminal output (cli.py).
Start here: Cue represents a timed subtitle; Candidate links examples to it.
All times are integer milliseconds; offsets count Python Unicode characters.
"""

import math
from dataclasses import asdict, dataclass

SCHEMA_VERSION = 1
PROMPT_VERSION = 1


class PipelineError(Exception):
    """Describe a failed external or persisted pipeline operation."""


class NeedsReview(PipelineError):
    """Describe unresolved decisions that prevent final delivery."""


@dataclass(frozen=True)
class Word:
    """Store aligned word text and its interval in milliseconds."""

    text: str
    start_ms: int
    end_ms: int


@dataclass(frozen=True)
class Cue:
    """Store one exact subtitle string and its source interval."""

    id: str
    start_ms: int
    end_ms: int
    text: str
    words: tuple[Word, ...] = ()


@dataclass(frozen=True)
class Span:
    """Locate a cue in the canonical plaintext using character offsets."""

    cue_id: str
    start: int
    end: int


@dataclass(frozen=True)
class Context:
    """Link a verbatim example and its highlight to timed source cues."""

    id: str
    text: str
    highlight_start: int
    highlight_end: int
    cue_ids: tuple[str, ...]
    start_ms: int
    end_ms: int
    matches: tuple[int, ...] = ()
    error: str = ""


@dataclass(frozen=True)
class Candidate:
    """Store a Blitzer lemma, its unmodified frequency, and examples."""

    id: str
    term: str
    source_frequency: int
    contexts: tuple[Context, ...]


@dataclass(frozen=True)
class Card:
    """Store validated learner fields and at most two timed examples."""

    id: str
    word: str
    english: str
    notes: str
    frequency: int
    contexts: tuple[Context, ...]
    source: str
    media: tuple[str, ...] = ()


def milliseconds(value, label: str) -> int:
    """Convert finite nonnegative seconds to milliseconds or raise ValueError."""
    if type(value) not in (float, int) or not math.isfinite(value) or value < 0:
        raise ValueError(f"{label} must be finite nonnegative seconds")
    return round(value * 1000)


def integer(value, label: str, minimum: int = 0) -> int:
    """Require an actual integer at or above minimum, excluding booleans."""
    if type(value) is not int or value < minimum:
        raise ValueError(f"{label} must be an integer >= {minimum}")
    return value


def text(value, label: str, *, empty: bool = False) -> str:
    """Require text, rejecting blanks unless the caller permits them."""
    if not isinstance(value, str) or (not empty and not value.strip()):
        raise ValueError(f"{label} must be {'possibly empty ' if empty else ''}text")
    if "\x00" in value:
        raise ValueError(f"{label} contains a NUL character")
    return value


def encode(records) -> list[dict]:
    """Return JSON-ready dictionaries without changing the supplied records."""
    return [asdict(record) for record in records]


def load_cues(rows) -> list[Cue]:
    """Reconstruct cues and optional word alignments from stored dictionaries."""
    return [
        Cue(**{**row, "words": tuple(Word(**w) for w in row.get("words", []))})
        for row in rows
    ]


def load_context(row) -> Context:
    """Reconstruct one context with immutable cue and match collections."""
    return Context(
        **{
            **row,
            "cue_ids": tuple(row["cue_ids"]),
            "matches": tuple(row.get("matches", [])),
        }
    )


def load_candidates(rows) -> list[Candidate]:
    """Reconstruct candidates and their timed contexts from JSON records."""
    return [
        Candidate(**{**row, "contexts": tuple(map(load_context, row["contexts"]))})
        for row in rows
    ]


def load_cards(rows) -> list[Card]:
    """Reconstruct export cards and media references from JSON records."""
    return [
        Card(
            **{
                **row,
                "contexts": tuple(map(load_context, row["contexts"])),
                "media": tuple(row.get("media", [])),
            }
        )
        for row in rows
    ]
