"""Render shared Anki fields and validate export prerequisites.

Scope statement: own presentation common to all delivery formats.
Included: note fields, escaped bold examples, media references, note templates,
model identity, and validation of local media ownership.
Excluded: deciding cards (vocabulary.py), generating media (media.py), writing
TSV/packages (tsv.py/package.py), and network requests (anki.py).
Start here: fields returns the same learner-facing content for every format.
"""

import html
from pathlib import Path

from blitzline.storage import digest

FIELDS = [
    "Word",
    "English",
    "Sentence1",
    "Sentence2",
    "Notes",
    "Frequency",
    "Media1",
    "Media2",
    "Source",
    "BlitzlineId",
]
FRONT = "{{Word}}<br>{{Sentence1}}<br>{{Media1}}"
BACK = "{{FrontSide}}<hr id=answer>{{English}}<br>{{Sentence2}}<br>{{Media2}}<br>{{Notes}}<br><small>{{Frequency}} occurrences · {{Source}}</small>"
CSS = ".card { font-family: sans-serif; font-size: 22px; text-align: left; } b { color: #3972bb; } small { font-size: 12px; }"


def escaped(value: str) -> str:
    """Escape untrusted text and display embedded line breaks as HTML breaks."""
    return (
        html.escape(value)
        .replace("\r\n", "\n")
        .replace("\n", "<br>")
        .replace("\t", "&#9;")
    )


def highlighted(context) -> str:
    """Render exact source text with one locally controlled bold span."""
    left, right = context.highlight_start, context.highlight_end
    return (
        escaped(context.text[:left])
        + "<b>"
        + escaped(context.text[left:right])
        + "</b>"
        + escaped(context.text[right:])
    )


def fields(card) -> dict[str, str]:
    """Return shared Anki fields with escaped text and verified media names."""
    sentences = [highlighted(c) for c in card.contexts] + ["", ""]
    media = [f"[sound:{name}]" for name in card.media] + ["", ""]
    values = [
        escaped(card.word),
        escaped(card.english),
        sentences[0],
        sentences[1],
        escaped(card.notes),
        str(card.frequency),
        media[0],
        media[1],
        escaped(card.source),
        card.id,
    ]
    return dict(zip(FIELDS, values))


def media_paths(cards, directory: Path) -> list[Path]:
    """Require safe, existing nonempty media files for every card example."""
    paths = set()
    identities = set()
    for card in cards:
        if card.id in identities:
            raise ValueError(f"Duplicate card identity: {card.id}")
        identities.add(card.id)
        if len(card.media) != len(card.contexts):
            raise ValueError(f"Card {card.word} has missing media")
        for name in card.media:
            if Path(name).name != name or not name.startswith("blitzline-"):
                raise ValueError("Invalid media filename")
            path = directory / name
            if not path.is_file() or not path.stat().st_size:
                raise ValueError(f"Missing or empty media: {name}")
            paths.add(path)
    return sorted(paths)


def model_identity(name: str) -> int:
    """Return a stable Anki numeric model ID tied to its name and field schema."""
    return int(digest([name, FIELDS])[:8], 16) % (2**31 - 1) + 1
