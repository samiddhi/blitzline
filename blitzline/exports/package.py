"""Generate portable Anki packages using the optional genanki writer.

Scope statement: own .apkg construction without opening an Anki profile.
Included: shared note templates, stable deck/model/note identities, media
bundling, atomic package writes, and archive sanity checks.
Excluded: package-writer internals (genanki), TSV (tsv.py), AnkiConnect (anki.py),
and recording or transcript processing (media.py/transcript.py).
Start here: export_package returns the generated .apkg path. Install the
'apkg' extra to enable this delivery mode.
"""

import os
import zipfile
from pathlib import Path

from blitzline.exports.common import (
    FIELDS,
    fields,
    media_paths,
    model_identity,
)
from blitzline.records import PipelineError
from blitzline.storage import digest


def export_package(
    cards, media_dir: Path, destination: Path, settings: dict
) -> list[Path]:
    """Bundle validated notes and media with deterministic genanki identities."""
    try:
        import genanki
    except ImportError as error:
        raise PipelineError(
            "APKG export requires: pip install 'blitzline[apkg]'"
        ) from error
    paths = media_paths(cards, media_dir)
    model = genanki.Model(
        model_identity(settings["model"]),
        settings["model"],
        fields=[{"name": name} for name in FIELDS],
        templates=[
            {
                "name": "Vocabulary",
                "qfmt": settings["front_template"],
                "afmt": settings["back_template"],
            }
        ],
        css=settings["css"],
    )
    deck_id = int(digest(settings["deck"])[:8], 16) % (2**31 - 1) + 1
    deck = genanki.Deck(deck_id, settings["deck"])
    for card in cards:
        deck.add_note(
            genanki.Note(
                model=model,
                fields=list(fields(card).values()),
                guid=card.id,
                tags=["blitzline", "blitzline_id_" + card.id],
            )
        )
    package = genanki.Package(deck)
    package.media_files = [str(path) for path in paths]
    destination.mkdir(parents=True, exist_ok=True)
    path = destination / "cards.apkg"
    temporary = destination / ".cards.apkg.tmp"
    try:
        package.write_to_file(str(temporary))
        with zipfile.ZipFile(temporary) as archive:
            if (
                "collection.anki2" not in archive.namelist()
                or "media" not in archive.namelist()
                or archive.testzip()
            ):
                raise PipelineError(
                    "Generated Anki package failed archive verification"
                )
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)
    return [path]
