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
import re
import zipfile
from pathlib import Path

from blitzline.exports.model import (
    export_media,
    model_id,
    note_tags,
    render_fields,
    resolve_model,
)
from blitzline.records import PipelineError
from blitzline.storage import digest


def requirement_template(value: str) -> str:
    """Expose filtered fields to genanki's Mustache-based card requirement analysis."""

    def unfiltered(match):
        """Retain section tags and reduce Anki display filters to their source field."""
        reference = match.group(1).strip()
        if reference.startswith(("#", "^", "/")):
            return match.group(0)
        return "{{" + reference.split(":")[-1] + "}}"

    return re.sub(r"{{\s*([^{}]+?)\s*}}", unfiltered, value)


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
    definition = resolve_model(settings)
    paths = export_media(cards, media_dir, definition)
    model = genanki.Model(
        model_id(settings["model"], definition),
        settings["model"],
        fields=[{"name": f["name"]} for f in definition["fields"]],
        templates=[
            {"name": t["name"], "qfmt": t["front"], "afmt": t["back"]}
            for t in definition["templates"]
        ],
        css=definition["css"],
        sort_field_index=definition["sort_field"],
    )
    # genanki uses generic Mustache for requirements; it does not know Anki filters.
    # Analyze unfiltered references while retaining the original shipped templates.
    requirement_model = genanki.Model(
        model.model_id,
        model.name,
        fields=model.fields,
        templates=[
            {**t, "qfmt": requirement_template(t["qfmt"])} for t in model.templates
        ],
    )
    try:
        model._req = requirement_model._req
    except Exception:
        raise PipelineError(
            "Cannot determine card-generation requirements; each front must reference a note field"
        ) from None
    deck_id = int(digest(settings["deck"])[:8], 16) % (2**31 - 1) + 1
    deck = genanki.Deck(deck_id, settings["deck"])
    for card in cards:
        deck.add_note(
            genanki.Note(
                model=model,
                fields=list(render_fields(card, settings, definition).values()),
                guid=card.id,
                tags=note_tags(card, settings),
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
