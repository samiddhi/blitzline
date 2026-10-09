"""Export Anki-importable TSV together with its media files.

Scope statement: own portable text delivery and import instructions.
Included: tab-delimited writing, literal empty fields, round-trip verification,
media copying, and a short import guide.
Excluded: building cards (vocabulary.py), cutting clips (media.py), other export
formats (package.py/anki.py), and direct collection database changes.
Start here: export_tsv creates cards.tsv and a media subdirectory.
"""

import csv
import io
import shutil
from pathlib import Path

from blitzline.exports.model import (
    export_media,
    note_tags,
    render_fields,
    resolve_model,
)
from blitzline.storage import atomic_text, write_json


def export_tsv(cards, media_dir: Path, destination: Path, settings: dict) -> list[Path]:
    """Write UTF-8 TSV and copy only referenced media, preserving empty columns."""
    definition = resolve_model(settings)
    names = [f["name"] for f in definition["fields"]]
    paths = export_media(cards, media_dir, definition)
    destination.mkdir(parents=True, exist_ok=True)
    media = destination / "media"
    media.mkdir(exist_ok=True)
    copied = []
    for source in paths:
        target = media / source.name
        shutil.copy2(source, target)
        copied.append(target)
    rows = [
        [
            *render_fields(card, settings, definition).values(),
            " ".join(note_tags(card, settings)),
        ]
        for card in cards
    ]
    stream = io.StringIO(newline="")
    writer = csv.writer(stream, delimiter="\t", lineterminator="\n")
    writer.writerows(rows)
    raw = stream.getvalue()
    if list(csv.reader(io.StringIO(raw), delimiter="\t")) != rows:
        raise ValueError("TSV round-trip verification failed")
    path = destination / "cards.tsv"
    atomic_text(
        path,
        "#separator:tab\n#html:true\n#columns:"
        + "\t".join([*names, "Tags"])
        + "\n#tags column:"
        + str(len(names) + 1)
        + "\n"
        + raw,
    )
    model_path = destination / "note-type.json"
    write_json(
        model_path,
        {
            "name": settings["model"],
            "deck": settings["deck"],
            "language": settings.get("language", ""),
            "deck_profile": settings.get("deck_profile", ""),
            "fields": names,
            "templates": definition["templates"],
            "css": definition["css"],
            "sort_field": names[definition["sort_field"]],
        },
    )
    templates_dir = destination / "templates"
    templates_dir.mkdir(exist_ok=True)
    template_paths = []
    for index, template in enumerate(definition["templates"], 1):
        for side in ("front", "back"):
            template_path = templates_dir / f"{index}-{side}.html"
            atomic_text(template_path, template[side])
            template_paths.append(template_path)
    css_path = templates_dir / "style.css"
    atomic_text(css_path, definition["css"])
    guide = destination / "IMPORT.md"
    atomic_text(
        guide,
        "# Import into Anki\n\nCopy the files inside media/ to your Anki profile's collection.media directory.\n"
        "Select destination deck "
        + settings["deck"]
        + " and create note type "
        + settings["model"]
        + ".\n"
        "The complete definition is in note-type.json, with these fields in order:\n"
        + ", ".join(names)
        + ".\nCreate each listed card template in order using templates/N-front.html and N-back.html;\n"
        "paste templates/style.css into Styling. The JSON also contains their names and exact contents.\n"
        "Import cards.tsv as tab-separated text with HTML enabled and map all "
        + str(len(names))
        + " note fields; the final column maps to tags via the file header.\n\n"
        "TSV duplicate detection normally uses the note type's first field ("
        + names[0]
        + ").\n"
        "Choose import duplicate settings deliberately; identical words from different recordings can collide.\n"
        "APKG and AnkiConnect instead use stable source-linked identities.\n",
    )
    return [path, guide, model_path, css_path, *template_paths, *copied]
